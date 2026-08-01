# Commercial certification Phase D handoff

Date: 2026-08-01 KST

Status: Phase D in progress; not a commercial release certificate

Authoritative plan:
`docs/superpowers/plans/2026-07-29-authoring-runtime-commercial-completion.md`

## Outcome at this checkpoint

Phases A, B, and C are implemented. The authenticated product path can accept a
bounded Luna-medium authoring turn, persist an encrypted generation, expose a
safe preview, promote, approve, Apply, converge an exact deployment to Live,
serve the private-study-room recipe on the canonical Discord shard, suppress
duplicate interactions durably, preflight the complete action plan, and journal
and recover per-action effects.

Phase D is not complete. D1 restart and failure cohorts are accepted. D2 has
not yet passed its unique disposable-database and disposable-guild 17-step
sequence. D3 has not certified an exact GitHub merge candidate or merged-main
tree. These facts prohibit a commercial-ready or production-certified claim.

## Fixed safety boundary

- The model proposes authoring intent only. It has no promotion, approval,
  Apply, deployment, PostgreSQL, Discord mutation, or event-time authority.
- Runtime execution and recovery are deterministic. No LLM call occurs during
  deployment, interaction dispatch, or recovery.
- PostgreSQL is authoritative. Gateway and registry state are disposable
  serving projections.
- No PostgreSQL transaction or row lock spans a model or Discord network call.
- A route is not Live because the API, worker, gateway, or runtime process is
  ready. Live requires the exact product deployment and fresh serving evidence.
- An unknown Discord outcome is observed before retry. Unsupported correlation
  becomes durable `recovery_required`; it is never guessed or blindly replayed.
- Automatic compensation requires the exact journaled identity and preimage.
  No unrelated customer resource may be deleted.
- Credentials, database URLs, Keychain material, OAuth data, interaction
  tokens, transcripts, and payloads remain outside source, logs, and evidence.

## Exact source inventory

| Item | Current source contract |
| --- | --- |
| Rust crates | 48 manifests under `crates/` |
| Rust tools | 10 manifests under `tools/` |
| Workspace members | 58 |
| SQL migrations | 117 |
| Migration head | `202608010002_fix_runtime_interaction_effect_response_tail_scan_v1.sql` |
| Owned user-schema relations after bootstrap | 198 |
| Capability functions after bootstrap | 135 |
| API database pools | 14 core plus 1 isolated authoring writer |
| Runtime database pools | 5 |
| Application database credentials | 20 pairwise-distinct logins |
| Managed Keychain items | 28 |
| Keyrings | 3: product action, snapshot envelope, interaction-token envelope |
| Final integrated HBA | 15 rules |
| PostgreSQL CI manifest | 13 explicit serial commands |

Migration 117 is additive and changes no relation or capability count. It
preserves the response-tail scan function's signature, owner, ACL, language,
security-definer status, fixed search path, strictness, volatility, parallel
classification, and row estimate while replacing two invalid
`pg_catalog.greatest` references with PostgreSQL's unqualified `GREATEST`
expression. Its preflight binds the exact migration-116 checksum and prior
function and manifest identities.

## Product and runtime behavior

### Authoring

The API composes authoring independently from its fourteen-pool core. General
readiness may stay green when the writer or loopback Codex worker is absent,
invalid, saturated, or unavailable. The current process emits one redacted
startup classification: `starring_api_authoring_status=ready` or
`starring_api_authoring_status=unavailable`.

Unavailable authoring fails closed with `dependency_unavailable`; bounded
capacity rejection is `authoring_saturated`; bounded dependency expiry is
`dependency_timeout`; invalid worker output is `upstream_invalid_response`.
Core product routes keep only their own authority. Authoring cannot borrow a
core pool, hot-add dependencies, or bypass worker validation. Recovery requires
a controlled restart after the writer and worker independently pass preflight.

### Serving and duplicate receipts

The runtime owns one canonical shard and five least-privilege pools. Process
readiness proves current process-wide authorities and supervisors, not a route.
The exact product deployment projection is the route authority.

`GET /health/interactions` is a loopback-only, process-local aggregate. Its
receipt-acquired, completed/in-flight/terminal/recovery-required duplicate,
persistence-failure, authority-rejection, and in-flight counters contain no
tenant, guild, installation, route, interaction, user, payload, token, or effect
identity. Counters reset on process restart, so retain before/after deltas only
for one process and pair them with durable and external resource evidence.

### Effect recovery

The effect journal has 22 capability functions and four tables. Eleven
functions are exported to the runtime interaction role; eleven helpers remain
owner-only. The periodic supervisor runs every 15 seconds with bounded pages,
concurrency, scan time, candidate time, and database-enforced attempt budgets.
Three consecutive failed sweeps or a stopped supervisor makes serving
revalidation fail closed.

Routine operators use only
`ops/postgres/staging-runtime-interaction-effect-inspection.sql`. It validates
the exact database, cluster acknowledgement, 117-entry ledger, migration
checksum, function identities, owner, ACL, and schema manifests, then emits
only block code, action kind, count, and oldest/newest timestamps. It emits no
customer or Discord identity. A zero-row result means no recovery-required
effect existed in that repeatable-read snapshot. Direct receipt/effect table
edits and operator-triggered deletion in response to the projection are
forbidden. Only the runtime's deterministic bounded compensation may delete an
exact journaled resource.

## D1 evidence

The first staging restart drill correctly exposed no false readiness, but it
found two release-blocking defects:

1. A resumed successor required the predecessor's exact ingress
   acknowledgement to remain unexpired, even though a process restart must
   rotate an exact expired prior-process acknowledgement.
2. The migration-116 response-tail scan used the nonexistent qualified
   `pg_catalog.greatest` function. The recovery supervisor failed each scan and
   serving readiness closed after the bounded three-sweep threshold.

The first failed candidate was rolled back without opening readiness. Source
fixes include:

- `60e69eb`: an exact resumed successor may rotate the expired predecessor
  acknowledgement only when every digest, revision, gateway, fence, gate, and
  finalizer identity matches and the predecessor observation predates the fresh
  owner expiry.
- migration 117: an exact, manifest-guarded correction of the response-tail
  scan.
- `cd84863`: composed failure tests for database-unavailable-before-claim and
  Discord-preflight-unavailable-before-effect boundaries.

The corrected immutable candidate at
`b4f2bb09f4997c2fda33ddef6a1175e642ca19ba` is now active. Staging has all 117
migrations, 198 owned user-schema relations, and 135 capability functions. The
effect ACL backfill and redacted inspection passed with zero recovery-required
groups.

Graceful restart rotated exact process authority without opening readiness
early. A forced `SIGKILL` successor reacquired authority in 66 seconds after
bounded stale-authority attempts, then held exact PID, liveness, readiness, and
fresh acknowledgement evidence for 40/40 samples over 83 seconds with zero
runtime failure codes. The deterministic and PostgreSQL matrix covers every D1
checkpoint and required failure, including duplicate HTTP and Discord delivery,
writer-fence, authority, binding, owner, controller, gateway, database, and
indeterminate-effect boundaries. Exact commands and identities are retained in
`docs/superpowers/measurements/2026-08-01-runtime-phase-d1.md`.

D1 is accepted. It is not the D2 disposable-guild certificate or the D3
merge-candidate and merged-main certificate.

## Backup, restore, shutdown, and rollback

Before migration or capability-ACL mutation:

1. Close public ingress.
2. Stop API and runtime and prove zero client backends and prepared
   transactions across the dedicated cluster.
3. Record the exact source and binary identities and migration ledger.
4. Create a mode-`0600` PostgreSQL 16 custom-format dump through the fixed
   Keychain-to-temporary-`PGPASSFILE` boundary.
5. Record only backup ID, byte count, SHA-256, and `pg_restore --list` success.
6. Restore into an isolated PostgreSQL 16 target, diff the complete migration
   ledger, and run ownership, manifest, and capability readiness checks.
7. Delete the disposable restore only after the drill result is retained.

Never restore over the active staging database, invent reverse SQL, or start an
older binary against an unrecognized function manifest. A full cluster rollback
uses the archived PGDATA path in the integrated cutover runbook. After restore,
public ingress stays closed until HBA, role, Keychain-reference, API, runtime,
route, and serving checks all pass.

API shutdown closes readiness and listener, drains HTTP work for at most 15
seconds, and closes all pools within a separate 15-second bound. Runtime
shutdown seals readiness, drains bounded work, releases owner authority, closes
Discord, joins supervisors, and closes pools within its 30-second process
deadline. Binary rollback changes only the executable; it does not rewind a
deployment, receipt, effect journal, route, or Discord resource.

## Remaining certification order

1. Run D2 from a unique disposable database and a newly created disposable
   Discord guild through all 17 steps, including duplicate delivery,
   indeterminate-effect reconciliation, replacement, disconnect, and complete
   cleanup. The retained B6 guild and installation do not satisfy D2.
2. Create the final PR, update onto current main, fetch GitHub's merge candidate,
   and run the complete local, JavaScript, 13-command PostgreSQL, and D2 gates on
   that exact tree.
3. Merge only while the base is unchanged, require the merged tree to equal the
   certified merge-candidate tree, and require both `checks` and `postgres` push
   jobs on the exact main merge commit.
4. Replace this in-progress handoff with final exact evidence identities and
   update the plan ledger only after every gate is true.

## Explicit non-claims and support limits

- No frontend exists.
- Only `starring.private_study_room@1` is implemented as a product recipe.
- Typed-planner handoff, arbitrary Discord games, multi-shard serving,
  multi-host high availability, non-Discord adapters, and a durable asynchronous
  authoring queue remain deferred.
- B6 proves a bounded standing-fixture path, not D2 cleanup or commercial
  certification.
- The Luna V15 232/232 matrix proves bounded serial authoring quality for its
  pinned source and worker, not current-CLI equivalence, concurrency, soak,
  quota behavior, or Discord execution.
- Phase C tests and the migration-117 regression do not replace live fault
  injection.
- Public Cloudflare reachability is not product readiness and does not weaken
  OAuth, session, CSRF, tenant, installation, fresh-authority, approval, Apply,
  deployment, or route gates.

## Source map

- Current state: `CURRENT_STATE.md`
- Commercial completion plan:
  `docs/superpowers/plans/2026-07-29-authoring-runtime-commercial-completion.md`
- Phase A evidence:
  `docs/superpowers/measurements/2026-07-30-authoring-milestone-a6.md`
- Phase B evidence:
  `docs/superpowers/measurements/2026-07-31-runtime-milestone-b6.md`
- Phase C evidence:
  `docs/superpowers/measurements/2026-08-01-runtime-milestone-c3.md`
- Phase D1 evidence:
  `docs/superpowers/measurements/2026-08-01-runtime-phase-d1.md`
- API operations:
  `docs/superpowers/runbooks/2026-07-19-production-control-plane-cutover.md`
- Integrated staging cutover:
  `docs/superpowers/runbooks/2026-07-29-macos-starring-integrated-staging-cutover.md`
- Runtime operations:
  `docs/superpowers/runbooks/2026-07-29-macos-starring-runtime-staging-operations.md`
- Codex worker operations:
  `docs/superpowers/runbooks/2026-07-17-macos-codex-worker-operations.md`
