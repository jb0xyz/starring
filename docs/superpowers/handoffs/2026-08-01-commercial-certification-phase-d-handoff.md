# Commercial certification Phase D handoff

Date: 2026-08-01 KST

Source-of-truth refresh: 2026-08-04 KST

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
| Workspace Rust tools | 10 manifests under `tools/` |
| Workspace members | 58 |
| Total Rust tool manifests | 11, including standalone `d2-certification-transport` |
| Top-level tool directories | 15 |
| SQL migrations | 125 |
| Migration head | `202608040004_refresh_serving_pending_product_drain_readiness_v1.sql` |
| Owned user-schema relations after bootstrap | 198 |
| Capability functions after bootstrap | 137 |
| API database pools | 14 core plus 1 isolated authoring writer, maximum 15 |
| Runtime database pools | 5, default connection ceiling 10 |
| Application database credentials | 20 pairwise-distinct logins |
| Managed Keychain items | 28 |
| Keyrings | 3: product action, snapshot envelope, interaction-token envelope |
| Final integrated HBA | 15 rules |
| PostgreSQL CI manifest | 13 explicit serial commands |
| D2 Keychain boundary | 29 run-owned items plus 3 external read-only items |

The 117-migration and 135-function values below belong to the immutable D1
measurement and must remain historical. The current source adds eight ordered
migrations and two capability functions without changing the 198-relation
manifest. Migration 117 preserves the response-tail scan function's signature,
owner, ACL, language, security-definer status, fixed search path, strictness,
volatility, parallel classification, and row estimate while replacing two
invalid `pg_catalog.greatest` references with PostgreSQL's unqualified
`GREATEST` expression. The current head refreshes the fail-closed serving
pending-product drain readiness contract.

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
the exact database, cluster acknowledgement, 125-entry ledger, migration
checksum, function identities, owner, ACL, and schema manifests, then emits
only block code, action kind, count, and oldest/newest timestamps. It emits no
customer or Discord identity. A zero-row result means no recovery-required
effect existed in that repeatable-read snapshot. Direct receipt/effect table
edits and operator-triggered deletion in response to the projection are
forbidden. Only the runtime's deterministic bounded compensation may delete an
exact journaled resource.

The D2 sealed inspector is a separate candidate-only boundary. Its
`authoring`, `live`, `interaction`, `duplicate`, `restart`, `reconciliation`,
`replacement`, `precleanup`, and `absence` checkpoints run read-only
repeatable-read observations and emit only
closed versioned envelopes. The certification transport exposes a sorted,
bounded resource inventory for manifest-owned roles, channels, and panel
messages with Created-to-Deleted history and one canonical digest. Operators
may retain those envelopes and digests, but not database URLs, row payloads,
interaction tokens, effect inputs, effect preimages, cookies, CSRF material,
prompts, transcripts, or RuleSet JSON.

Step 9 is an exact three-source join. Chrome supplies the visible manifest
guild, prefix, actor, distinct create and join interaction IDs, joined role,
resource inventory, and affirmative create-response, join-response, private
channel, role-assignment, and join-panel observations. The sealed database
checkpoint independently requires completed create and join receipts, the same
manifest actor, one successful created-role membership in each path, the exact
created instance, and one successful ephemeral acknowledgement per receipt.
The transport inventory must match the one role, one channel, and one panel
message exactly.

Step 15 is complete only after one durable partition operation causes the
strict public `200/200`, product `pending`, operational `pending`, runtime
`live`, serving `disconnected`, `runtime_gateway_disconnected`, retryable
Live-loss projection and one durable heal completion restores the same
transport instance with readiness 200, partition false, and every
duplicate or indeterminate arm and claim false. Step 16 starts only after the
coordinator's step-15 completion is sealed and binds teardown to a durable
freeze intent and frozen resource-inventory digest. Standalone teardown writes
an abort tombstone and permanently disqualifies the run. Step 17 is later than
the exact step-16 completion and joins sealed database absence, Chrome prefix
absence, Chrome guild deletion, and orchestrator absence.

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

1. Require a canonical `github.com` HTTPS or SSH origin whose owner and
   repository exactly match the D3 invocation. Freeze the final PR head and
   current `main` base, fetch GitHub's generated merge candidate, and build
   every immutable D2 artifact from that exact tree.
2. Create a unique disposable database, resource prefix, application, guild,
   and `community_hub`; prove no prior owner, smoke process, standing port,
   launchd label, Keychain service, or protected resource is reused.
3. Start only the manifest-bound database, transport, worker, API, runtime, and
   tunnel. Complete OAuth once and record the strict redacted authentication
   envelope.
4. Run the 17 D2 steps in order: substrate identity; absence of a prior owner;
   exact services; OAuth; one-shot authoring bounded by one exact worker request;
   public PreviewReady joined to its encrypted generation; then, only after the
   step-6 coordinator completion, reload, confirm, promote, approve, and Apply;
   RuntimePending-to-Live; real create and join; duplicate
   delivery with one effect; runtime restart; route and instance reconstruction;
   one indeterminate effect with reconciliation; drain and replacement;
   gateway partition with Live loss; resource teardown; and total absence.
5. Before each receipt, join the browser envelope, sealed read-only database
   checkpoint, transport resource inventory, fault snapshot, and visible
   Discord observation required by that step. Never infer a receipt from
   process readiness or a single evidence source.
6. For teardown, first seal step 15 and write the durable freeze intent. Delete
   only identities present in that frozen manifest-bound Created
   inventory, accept only the exact success or provider-not-found result, mark
   them Deleted, delete the disposable guild through the human boundary, then
   require zero unresolved operations, receipts, journals, routes, instances,
   roles, channels, messages, run-owned Keychain items, launchd jobs, database,
   and run root. The retained B6 guild and installation do not satisfy D2.
7. On the same exact merge-candidate tree, run the immutable ordered D3 command
   manifest: formatting, workspace build and
   tests, Clippy, unsafe-dev smoke build, Codex-worker checks, SLO checks,
   Promptfoo install/audit/check, the standalone D2 Python coordinator,
   product-driver Node, and certification-transport format/test/Clippy suites,
   plus all 13 serial PostgreSQL commands. Bind the completed D2 cohort after
   those 29 commands as separate evidence. A changed,
   missing, added, duplicated, or reordered command invalidates the gate.
8. Merge only while the PR base and head remain frozen. Require the resulting
   `main` tree to equal the certified merge-candidate tree byte-for-byte, then
   require green `checks` and `postgres` push jobs on that exact main merge
   commit. Any base, head, tree, or evidence drift restarts candidate
   certification.
9. Mark D2, D3, D4, and the final PR ledger entries complete only after every
   condition above is true.

Final identities are external evidence because a tracked document cannot bind
its own final tree without changing that tree. The machine-updated release
record is
the D3 terminal record `<D3_RUN>/final.json`, created only by
`tools/d3-certification/d3_certification.py finalize`. `<D3_RUN>` is the
directory containing the absolute D3 `state.json` path returned by `prepare`.
It must fill these fields from immutable run and GitHub evidence:

| External evidence field | Authoritative source | Current value |
| --- | --- | --- |
| D2 run ID and receipt-chain head | completed D2 run directory | pending |
| PR number, head commit, and base commit | GitHub PR metadata | pending |
| merge-candidate commit and tree | GitHub `refs/pull/<n>/merge` | pending |
| D2 certified commit and tree | immutable D2 manifest | pending |
| merged-main commit and tree | GitHub merge result | pending |
| one Actions run ID containing green `checks` and `postgres` push jobs | GitHub Actions | pending |

`pending` means no certificate exists. It must never be replaced by a guessed
or locally synthesized identity. This tracked handoff and the plan checkboxes
are the immutable pre-certification snapshot. Do not edit them after merge to
replace `pending` or flip D2, D3, D4, or final-PR boxes; a valid sealed
D3 terminal record supersedes that snapshot as the sole terminal completion
record.

## Explicit non-claims and support limits

- No frontend exists.
- Only `starring.private_study_room@1` is implemented as a product recipe.
- Typed-planner handoff, arbitrary Discord games, multi-shard serving,
  multi-host high availability, non-Discord adapters, and a durable asynchronous
  authoring queue remain deferred.
- This certificate is scoped to one Mac mini, one canonical shard, and
  `starring.private_study_room@1`. It does not certify full production secret
  isolation from the logged-in user boundary or a high-volume production SLO.
- Sustained load and soak, disaster-recovery restore, non-interactive host
  reboot, and public-ingress capacity are separate production-rollout
  certificates. Their bounded code paths and runbooks are part of Backend V1,
  but Phase D does not claim those external cohorts passed.
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
