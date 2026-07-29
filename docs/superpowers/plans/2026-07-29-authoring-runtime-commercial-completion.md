# Trusted Authoring and Commercial Runtime Completion Implementation Plan

Date: 2026-07-29

Status: accepted and ready for execution

Planning baseline: `main` at `913e0eb`

Baseline inventory at that commit:

- 46 crate manifests under `crates/`
- 9 Rust tool manifests under `tools/`
- 55 workspace members
- 88 ordered SQL migrations
- 14 API database roles and pools
- 5 runtime database roles and pools

`CURRENT_STATE.md` contains older inventory counts. Correcting those counts is
part of the first source-of-truth update. Creating the reusable Codex worker
client in Task A1 raises the crate-manifest count to 47 unless implementation
evidence justifies a different extraction boundary.

## Estimate contract

The accepted estimate is:

- Staging actual operation: **3–5 focused workdays**
- Commercial-safe completion, including duplicate interaction prevention and
  partial external-failure recovery: **5–8 focused workdays total**

These are implementation-effort ranges, not calendar guarantees. They include
focused compilation and validation time. They do not assume that an external
provider, GitHub Actions, Discord, PostgreSQL, Keychain, or macOS code-signing
service is continuously available.

The estimate must not be silently compressed into a one-day commercial
completion claim. It may be revised only when scope or observed evidence
changes, and the reason must be recorded.

## Goal

Complete the missing production path from authenticated natural-language
authoring to deterministic Discord interaction serving:

```text
authenticated human turn
  -> Luna-medium Design Harness
  -> deterministic validation and simulation
  -> encrypted PostgreSQL authoring generation
  -> preview
  -> existing promote, approve, and Apply flow
  -> Requested runtime deployment
  -> exact hydration, preflight, panel reconciliation, and certification
  -> Live serving route
  -> deterministic Discord interaction execution
  -> durable duplicate suppression and partial-failure recovery
```

The model remains a design-time proposer. It never receives approval, Apply,
runtime, database, Discord mutation, or deployment tools. Event-time and
deployment-time model calls remain forbidden.

## Current baseline

The implementation already contains:

- A passing Luna-medium V4 authoring harness for
  `starring.private_study_room@1`
- Deterministic Intent IR, Recipe Compiler, validation, simulation, preview,
  and semantic identity
- Immutable encrypted authoring generation schema and atomic authorized
  promotion reads
- Authenticated OAuth, session, CSRF, fresh Discord authority, promote,
  approve, reject, Apply, and deployment-status product boundaries
- Durable Requested-to-Live convergence state machines, exact target
  hydration, strict panel reconciliation, V2 certification, serving lease,
  heartbeat, interaction routing, and route registry components
- A production-shaped `starring-api` staging process
- A `starring-runtime` process that safely reaches paused connected
  empty-open

The missing production connections are:

1. A trusted writer from Design Harness output to PostgreSQL authoring
   generations
2. An authenticated conversational authoring HTTP surface
3. A runtime control loop that advances real Requested deployments to Live
4. A canonical-shard interaction dispatcher in `starring-runtime`
5. Durable Discord interaction duplicate prevention
6. Whole-plan preflight and recovery for partial Discord side effects

## Scope

### Included

- One-shot and conversational authoring through the same HTTP turn endpoint
- The existing `starring.private_study_room@1` recipe
- Luna-medium through the loopback ChatGPT-auth Codex worker
- Encrypted PostgreSQL authoring generations
- Existing human preview, approval, and Apply boundaries
- Requested-to-Live production runtime composition
- Real Discord panel, button, modal, role, channel, permission, and response
  execution required by the current recipe
- Restart reconstruction, duplicate interaction suppression, deterministic
  preflight, effect journaling, reconciliation, and bounded compensation
- Least-privilege database capabilities and negative security tests
- Disposable-guild end-to-end certification

### Deferred

- Frontend implementation
- Administrative and installation-management APIs
- Additional product recipes
- General typed-planner handoff
- Arbitrary Discord games outside the current recipe contract
- Multi-shard Discord serving
- Multi-host high availability
- Non-Discord adapters
- A durable asynchronous authoring job queue

The initial authoring endpoint is synchronous and bounded. A durable
asynchronous queue is added only when measured queueing, disconnect loss,
concurrency, or high-availability requirements justify it.

## Global constraints

- No Rust source comments. `//`, `///`, `//!`, and block comments remain
  forbidden by repository policy.
- Do not change the model-facing safety boundary.
- Do not give `design-harness` SQL, Twilight, deployment, or Discord mutation
  dependencies.
- Keep `authoring-application` and `automation-runtime-worker` pure over
  injected ports.
- Do not hold a PostgreSQL transaction or row lock across a model or Discord
  network call.
- Secrets and provider endpoints come only from environment or Keychain
  composition. They never enter code, fixtures, commits, logs, or evidence.
- PostgreSQL remains authoritative. In-memory registry and gateway state are
  disposable serving projections.
- Do not weaken the existing empty-open invariant. Add typed serving states.
- Use one canonical Discord shard. Do not run `run_shared_gateway_v3` as a
  second shard beside the existing `starring-runtime` gateway.
- Preserve functional commit boundaries.
- Run focused tests per commit, milestone gates at major integrations, and one
  complete release gate at the final exact head.
- Keep local commits until the final release candidate. Create one final PR,
  require the PR head and generated merge candidate to be green, preserve the
  functional commits, record both identities, merge, and require the resulting
  main merge commit to pass CI.

## Fixed architecture decisions

### Synchronous bounded authoring V1

The initial authoring API waits for the bounded Design Harness turn and returns
its safe projection. This fits the current low-volume service and the
single-concurrency Codex worker.

The product API:

- Serializes the same in-flight authoring idempotency identity inside the
  single production process
- Rechecks exact idempotency replay and generation head after obtaining that
  keyed permit and before calling Luna
- Opens no database transaction during the model call
- Rechecks mutation authentication and fresh Discord authority after the call
- Commits the next generation with compare-and-set
- Returns an exact prior result after a lost HTTP response without calling
  Luna again

A process crash during an unfinished model call may require the caller to
retry. This is accepted for V1. A committed result is never recomputed on
retry.

### Separate conversation authority

A new conversation application owns only:

- Authentication and CSRF mutation authentication
- Fresh `Author` authority
- Authoring session load and generation compare-and-set
- Design Harness execution
- Safe authoring projection

It cannot promote, approve, Apply, activate, deploy, or mutate Discord.

### Dedicated writer database capability

The authoring writer receives its own direct-login role and database pool. It
may execute only fixed-search-path security-definer functions for exact
authoring load, replay lookup, commit, database identity, and readiness. It
receives no relation privileges.

### Extend the existing production lifecycle

The repository already has the process-level production lifecycle under
`automation-runtime-worker::production_lifecycle` and per-slot
`Staged`, `Serving`, and `Draining` registry states. The implementation extends
those existing authorities. It does not introduce a competing global
lifecycle.

Startup already establishes the process-level path:

```text
FixedPoint
  -> ProductionHandoff
  -> AdmissionAcknowledging
  -> OpenProduction
  -> Emergency or Shutdown
```

`RuntimeEmptyOpenProcessV2` already represents OpenProduction with an empty
registry. A new successor consumes `RuntimeEmptyOpenProcessV2` once, preserves
the same OpenProduction coordinator generation and admission authority, and
adds the bounded per-slot serving supervisor. It must not replay FixedPoint,
ProductionHandoff, AdmissionAcknowledging, or gateway startup.

Per-slot convergence, staging, replacement, certification, serving, and
draining run beneath that existing OpenProduction authority. Different slots
may converge concurrently within the global bound without turning the process
itself into one global `Staged` or `Serving` state.

Empty-open continues to require an empty registry. No route may be serving
merely because Discord is connected, a panel exists, or the product deployment
is Applied.

### Two admission barriers

Barrier A closes admission and drains the prior route before destructive route
replacement.

Barrier B binds local route activation, the exact gateway connection epoch,
owner lease, certification reservation, durable Live commit, serving monitor,
and ingress acknowledgement.

### Durable event and effect identities

The commercial runtime uses:

- A durable interaction receipt keyed by the exact Discord application and
  interaction identity
- A deterministic complete action-plan digest
- A durable per-action effect journal
- Explicit known-success, known-failure, and indeterminate external outcomes
- Observation and reconciliation before retrying an indeterminate effect

## Delivery milestones

| Milestone | Cumulative estimate | Product result |
| --- | ---: | --- |
| A. Trusted authoring | 1–1.5 focused days | Natural language reaches an encrypted PreviewReady generation |
| B. Staging runtime | 3–5 focused days | Existing recipe reaches Live and executes in a disposable Discord guild |
| C. Runtime hardening | 4.5–7 focused days | Duplicate and partial-failure behavior is implemented but not yet release-certified |
| D. Commercial certification | 5–8 focused days total | Restart, failure, E2E, complete local gates, PR CI, and merged-main CI are certified |

Incremental planning budget:

- Phase A: 1–1.5 focused days
- Phase B: 2–3.5 additional focused days
- Phase C: 1.5–2 additional focused days
- Phase D: 0.5–1 additional focused day

Task-level planning budget:

| Task | Focused-day target |
| --- | ---: |
| A1 worker client extraction | 0.1–0.2 |
| A2 conversation application | 0.2–0.25 |
| A3 encrypted writer and migration | 0.3–0.4 |
| A4 capability provisioning | 0.1–0.15 |
| A5 HTTP composition | 0.2–0.3 |
| A6 authoring gate | 0.1–0.2 |
| B1 lifecycle successor | 0.2–0.4 |
| B2 convergence and hydration | 0.5–0.8 |
| B3 route replacement and panels | 0.4–0.6 |
| B4 certification and monitor | 0.5–0.8 |
| B5 dispatcher and teardown | 0.3–0.6 |
| B6 staging E2E | 0.1–0.3 |
| C1 receipts and token envelope | 0.4–0.6 |
| C2 complete-plan preflight | 0.3–0.4 |
| C3 effect recovery | 0.8–1 |
| D1 failure cohort | 0.2–0.3 |
| D2 final E2E | 0.15–0.25 |
| D3 complete gates and merge evidence | 0.1–0.25 |
| D4 source-of-truth and operations | 0.05–0.2 |

The 5–8-day commercial range includes Phase D. It assumes reuse of the
accepted runtime state machines, no new product recipe, one Mac mini, one
Discord shard, one staging guild, focused per-commit validation, and no
material external-provider outage. Scope expansion or evidence that an
accepted substrate cannot be reused requires an explicit estimate revision.

Phase C has the lowest confidence because Discord does not provide a universal
idempotency or observation key for every create action. Re-estimate after B6
using the proven action-by-action observation matrix. If any task exceeds its
upper target materially, revise the range with evidence; do not recover time by
removing security, restart, failure, or release gates.

## Progress ledger

- [ ] A1. Extract the reusable Codex worker client
- [ ] A2. Add the pure conversation application
- [ ] A3. Add authenticated snapshot encryption and writer persistence
- [ ] A4. Provision the writer capability
- [ ] A5. Expose authenticated authoring HTTP routes
- [ ] A6. Pass the authoring milestone gate
- [ ] B1. Extend the existing production lifecycle for route serving
- [ ] B2. Compose the convergence lane and exact hydration
- [ ] B3. Replace prior routes and reconcile panels
- [ ] B4. Finalize V2 certification and serving monitor
- [ ] B5. Dispatch interactions on the canonical shard
- [ ] B6. Pass the staging end-to-end gate
- [ ] C1. Add durable interaction receipts
- [ ] C2. Add complete deterministic action-plan preflight
- [ ] C3. Add effect journal, reconciliation, and bounded compensation
- [ ] D1. Complete restart and failure cohorts
- [ ] D2. Pass the final disposable-guild product E2E
- [ ] D3. Pass the final complete gate
- [ ] D4. Update source-of-truth and operations
- [ ] Open one final PR, certify its merge candidate, merge, and certify the
  resulting main merge commit

## Phase A: trusted authoring

### Task A1: Extract the reusable Codex worker client

Files:

- Create `crates/design-harness-codex-worker-client/`
- Modify workspace `Cargo.toml`
- Modify `tools/design-harness/`
- Preserve `tools/codex-worker/`

Work:

- Move the loopback Codex worker protocol out of the CLI-only module
- Preserve exact provider, model, reasoning effort, authentication mode,
  Codex version, worker identity, single-frontier, usage, and strict-response
  checks
- Preserve loopback-only URL enforcement
- Preserve secret-redacted errors and metrics
- Reuse the new client from `tools/design-harness`
- Add dependency guards forbidding SQL, Twilight, product-control, promotion,
  and runtime dependencies

Focused validation:

- Existing design-harness CLI protocol tests remain unchanged in behavior
- Non-loopback URLs fail
- Wrong model, worker version, Codex version, frontier count, or response shape
  fails closed
- Worker tokens do not appear in Debug, error, metric, snapshot, or fixture
  output

Commit:

```text
refactor(harness): extract reusable Codex worker client
```

### Task A2: Add the pure conversation application

Files:

- Add `crates/authoring-application/src/conversation/`
- Modify `crates/authoring-application/src/authority.rs`
- Modify `crates/authoring-application/src/lib.rs`
- Modify `crates/authoring-application-discord/src/adapter.rs`
- Modify `crates/authoring-application-discord/src/evidence.rs`
- Add focused application tests
- Add focused Discord authority encoding and lifetime tests

Work:

- Add `CapabilityV1::Author`
- Treat Author as a fresh write capability
- Keep the existing guild-owner or effective `ADMINISTRATOR | MANAGE_GUILD`
  policy
- Do not require Apply runtime-environment evidence for Author
- Give Author its own stable capability encoding and digest domain
- Include Author in the write-evidence lifetime and keep
  `requires_runtime_environment(Author)` false
- Define bounded start-or-advance turn commands
- Define authoring session load and commit ports
- Define safe turn projections
- Accept only installation selector, opaque session ID, expected generation,
  idempotency identity, and human message
- Derive actor, tenant, guild, binding map, authority revision, model, recipe,
  Draft, and candidate identity on the server
- Restore existing snapshots through the current Design Harness compatibility
  contract
- Run one-shot and multi-turn requests through the same `run_burst` path
- Persist successful `NeedsInput`, discussion, capability-gap, and
  PreviewReady states
- Do not persist a halted or structurally invalid turn
- Re-export `PreviewReadyArtifactV1` before accepting a PreviewReady projection

Required sequence:

1. Authenticate opaque product session
2. Verify CSRF mutation authentication
3. Obtain fresh Author authority
4. Derive the scoped writer idempotency identity
5. Acquire a process-local keyed single-flight permit for that identity
6. Check exact replay and the generation head
7. Return replay or conflict without acquiring model capacity when possible
8. Acquire the bounded model-capacity permit
9. Recheck exact replay and the generation head immediately before `run_burst`
10. Load the exact current generation and current bindings
11. Restore or initialize `DesignSession`
12. Execute the bounded human turn
13. Validate the snapshot and canonical safe projection
14. Recheck mutation authentication and fresh Author authority
15. Compare actor, scope, authority, bindings, and expected generation
16. Commit or return conflict without mutating the head
17. Release the keyed permit only after the exact terminal outcome is known

Focused validation:

- New session produces generation 1
- One-shot private study room can become PreviewReady
- A question turn persists and the next turn resumes it
- Custom details preserve the fixed one- or two-call V4 contract
- Capability gap remains non-deployable
- Halted output creates no generation
- Authority or binding drift after the model call creates no generation
- Stale expected generation creates no generation
- Exact replay performs zero new model calls
- Concurrent same-key requests perform exactly one model call and the waiter
  returns exact replay or semantic conflict
- Different idempotency keys remain bounded only by the configured model
  capacity
- No application constructor can receive promotion, Apply, runtime, or Discord
  mutation ports

Commit:

```text
feat(authoring): add trusted conversation application
```

### Task A3: Add authenticated snapshot encryption and writer persistence

Files:

- Modify `crates/authoring-application-postgres/src/envelope.rs`
- Modify `crates/authoring-application-postgres/src/envelope/xchacha.rs`
- Add `crates/authoring-application-postgres/src/authoring_writer/`
- Add the next ordered migration after
  `202607290001_persist_runtime_ingress_open_acknowledgement_v2.sql`
- Add migration guards and ignored PostgreSQL integration tests

Work:

- Add a separate encryption port rather than broadening the snapshot-reader
  authority
- Encrypt only with the active XChaCha20-Poly1305 key
- Generate a fresh 24-byte nonce
- Reuse exact authenticated metadata construction
- Keep plaintext in zeroizing memory
- Keep active and retired keys available for read and replay coverage
- Use the existing immutable generation, contiguous sequence, head foreign
  key, lifecycle, binding fingerprint, authority revision, and
  `writer_request_digest` constraints
- Bind `writer_request_digest` to tenant, installation, principal, session,
  and client idempotency key through the product-action HMAC keyring
- Add legacy-compatible nullable generation columns named
  `writer_semantic_request_digest`, `writer_digest_key_id`, and
  `writer_digest_key_fingerprint`
- Add legacy-compatible nullable generation columns named
  `safe_turn_projection` and `safe_turn_projection_digest`
- Require all five writer metadata columns together for every trusted writer
  commit while preserving readability of pre-writer historical rows
- Derive `writer_semantic_request_digest` as a domain-separated HMAC over the
  canonical expected generation and human message rather than storing a raw
  low-entropy prompt hash
- Use that digest so same-key same-payload replay is distinct from same-key
  different-payload conflict
- Store the active product-action digest key ID and fingerprint and accept
  active and retired candidate identities during replay lookup
- Persist one bounded canonical safe turn projection and its digest
- Bind the safe projection digest into authenticated snapshot metadata
- Return the stored safe projection for exact replay rather than
  reconstructing an HTTP response from a snapshot under newer code
- Never persist a raw model response, backend error, unvalidated projection,
  or secret in the safe projection
- Add fixed functions for database identity, exact load and replay lookup,
  atomic commit, and key-coverage readiness
- Perform first session and generation-1 creation atomically
- Perform N-to-N+1 generation advancement atomically
- Recheck active session, principal, installation, authority revision,
  authority digest, binding fingerprint, and expected head inside commit
- Return exact replay, idempotency conflict, generation conflict, authority
  conflict, or committed successor as closed outcomes

Focused validation:

- Encryption round trip and active-key selection
- Retired-key decryption and replay lookup
- Ciphertext, nonce, key ID, AAD, tenant, installation, session, generation,
  binding, and authority tampering fails
- First write is atomic
- N-to-N+1 compare-and-set is atomic
- Two concurrent writers from one head produce one successor
- Same key and same payload returns exact replay
- Same key and different payload returns conflict
- Lost commit response followed by replay returns the committed generation
- Exact replay returns the byte-equivalent stored canonical safe projection
- A projection that differs from its digest or authenticated snapshot metadata
  fails closed
- Missing or partially populated trusted-writer metadata fails
- Active and retired digest key candidate identity and fingerprint coverage is
  exact
- Direct relation SELECT, INSERT, UPDATE, and DELETE fail for the writer role
- Every function outside the writer allowlist fails for the writer role
- API, reader, PUBLIC, and unrelated roles cannot use the writer functions
- Migration fresh install, upgrade, collision, ACL drift, and postflight
  checks fail closed

Commit:

```text
feat(postgres): add encrypted authoring generation writer
```

### Task A4: Provision the writer capability

Files:

- Modify `ops/postgres/staging-api-role-bootstrap.sql`
- Modify `ops/postgres/staging-api-role-enable.sql`
- Modify `tools/starring-db-bootstrap/`
- Modify `tools/starring-staging-provisioner/`
- Modify `tools/starring-api/src/config.rs`
- Modify `tools/starring-api/src/secret.rs`
- Modify `tools/starring-api/src/composition.rs`
- Modify `ops/macos/local.starring.api.staging.plist`
- Modify relevant staging runbooks and capability manifests

Work:

- Add the fifteenth API database role and pool
- Use the fixed role name `starring_authoring_session_writer`
- Add the Keychain account
  `starring-api.staging/database.authoring-session-writer`
- Add loopback Codex worker URL and token references to the staging API plist
- Keep the worker token in its existing Keychain boundary and out of generated
  plist values, logs, and provisioner receipts
- Update exact credential counts and pairwise-distinct role checks
- Update bootstrap, enable, readiness, rollback, and cleanup inventories
- Update fixed inventory tests in the staging provisioner and launchd
  manifests
- Keep the writer pool out of all existing reader, decision, and promotion
  adapters
- Make incomplete writer provisioning degrade only authoring unless a fixed
  release manifest explicitly requires the complete product surface

Focused validation:

- Exact role identity and database UUID
- Complete positive and negative capability matrix
- Pairwise-distinct login roles
- No role membership
- No PUBLIC or default-privilege leak
- Provision, exact replay, partial provision, rollback, and restored-cluster
  behavior

Commit:

```text
ops(postgres): provision trusted authoring writer capability
```

### Task A5: Expose authenticated authoring HTTP routes

Files:

- Add authoring DTOs and facade methods in `crates/product-control-http/`
- Add authoring routes in `crates/product-control-http/src/router.rs`
- Modify `tools/starring-api/src/facade.rs`
- Modify `tools/starring-api/src/composition.rs`
- Modify `tools/starring-api/src/config.rs`
- Add API integration tests

Routes:

```text
POST /v1/installations/{installation_id}/authoring/sessions/{session_id}/turns
GET  /v1/installations/{installation_id}/authoring/sessions/{session_id}
```

POST request:

```json
{
  "expected_generation": 0,
  "message": "스터디룸을 만드는 버튼을 만들어줘"
}
```

POST requires:

- Product session cookie
- Exact allowed Host and Origin
- CSRF token
- Valid Idempotency-Key
- Bounded installation ID, session ID, expected generation, and message

GET requires:

- Product session cookie
- Exact allowed Host and the existing authenticated-read boundary
- Fresh Read authority for the exact installation
- Exact session owner, tenant, installation, and principal scope
- Non-enumerating not-found behavior for every inaccessible or guessed scope
- Successful envelope decryption and active-or-retired key coverage
- `Cache-Control: no-store`
- The same closed safe-projection response validation as POST

The client cannot supply:

- Tenant or principal
- Discord actor or guild
- Authority revision or bindings
- RuleSet, Draft, snapshot, stage, candidate hash, or recipe version
- Model, provider, reasoning effort, tool definitions, or system prompt

POST response exposes only:

- Session ID and generation
- `created` or `exact_replay`
- Closed state such as `needs_input`, `preview_ready`, or `capability_gap`
- Bounded assistant question or response
- Safe Draft summary
- Server-rendered preview and candidate receipt only when PreviewReady

GET uses a separate read DTO:

- Session ID and observed generation
- Closed state such as `needs_input`, `preview_ready`, or `capability_gap`
- The exact stored canonical safe projection
- No `created`, `exact_replay`, or other mutation disposition

Work:

- Add an authoring-only timeout derived from the maximum V4 model-call count
  and the per-worker-call timeout
- Keep the existing general API timeout unchanged
- Add a capacity semaphore equal to configured worker concurrency, initially 1
- Add a bounded keyed single-flight map for in-process duplicate authoring
  identities
- Check replay after the keyed permit and recheck replay and generation after
  the capacity permit immediately before the worker call
- Return a bounded retryable saturation response with `Retry-After`
- Do not treat capacity admission as a customer quota or public rate limit
- Keep general product readiness independent from Codex worker health
- Report authoring degraded status only at the authoring boundary
- Validate response bodies so snapshot bytes, ciphertext, transcript internals,
  secrets, and raw backend errors cannot cross HTTP

Focused validation:

- Cookie, Host, Origin, CSRF, idempotency, and content type are mandatory
- Unknown JSON fields and oversized or invalid messages fail
- Unicode is accepted within scalar and byte bounds
- Control characters and ambiguous duplicate JSON fields fail
- Saturation does not call the worker
- Timeout cancels the turn and commits no generation
- Exact replay bypasses the worker and returns the same safe projection
- Concurrent exact duplicate POSTs perform one worker call
- Concurrent same-key different-payload POSTs return one success and one
  semantic conflict without a second worker call
- GET rejects cross-principal, cross-tenant, cross-installation, non-owner,
  revoked, expired, corrupt-envelope, and unavailable-key access without
  enumeration
- GET and POST responses are `no-store`
- Existing control routes keep their timeout and wire behavior

Commit:

```text
feat(api): expose trusted conversational authoring
```

### Task A6: Authoring milestone gate

Work:

- Run focused pure and adapter suites
- Run the dedicated authoring writer PostgreSQL suite serially
- Run affected package Clippy
- Execute one live Luna one-shot and one multi-turn case against staging
- Confirm encrypted generation storage
- Promote the exact PreviewReady generation through the existing endpoint
- Stop before automatic approval or Apply
- Update `CURRENT_STATE.md` facts and inventory counts

Milestone A acceptance:

- Natural language reaches a durable encrypted generation
- PreviewReady is independently revalidated
- Promotion consumes the exact stored generation
- No new deployment or Discord authority is exposed to the model

Commit:

```text
test(authoring): prove conversation to promotion vertical slice
```

## Phase B: staging runtime serving

### Task B1: Extend the existing production lifecycle for route serving

Files:

- Extend `crates/automation-runtime-worker/src/production_lifecycle/`
- Modify `tools/starring-runtime/src/process/`
- Preserve `RuntimeEmptyOpenProcessV2`
- Add focused lifecycle and dependency-guard tests

Work:

- Preserve the empty-registry requirement in EmptyOpen
- Add a serving-open successor that consumes `RuntimeEmptyOpenProcessV2`
  without replaying startup
- Preserve the exact existing OpenProduction coordinator generation, admission
  authority, gateway ownership, and ingress acknowledgement during that
  transition
- Keep process-level production authority separate from the registry's
  per-slot `Staged`, `Serving`, and `Draining` lifecycle
- Add typed route-set epochs under existing OpenProduction authority
- Preserve concurrent different-slot convergence under one bounded process
  authority
- Make stale generation, stale process, stale owner, illegal route presence,
  and illegal resume fail closed
- Ensure emergency and shutdown are monotonic and cannot reopen admission

Focused validation:

- A route cannot exist while the process still owns EmptyOpen authority
- The serving-open successor can be created only by consuming the exact
  EmptyOpen value once
- No FixedPoint, ProductionHandoff, AdmissionAcknowledging, owner acquisition,
  gateway start, or initial ingress-open step is replayed
- A per-slot Serving route cannot be entered from an unverified state
- Two different slots can progress without competing global lifecycle states
- Emergency and shutdown invalidate ordinary transition authority
- Stale observations and route epochs cannot advance state
- Recovery returns to an exact typed checkpoint

Commit:

```text
feat(runtime): extend production lifecycle for route serving
```

### Task B2: Compose the convergence lane and exact hydration

Files:

- Add pure orchestration modules under `automation-runtime-worker`
- Add `tools/starring-runtime/src/runtime_controller.rs`
- Modify runtime `config.rs`, `database.rs`, `registry.rs`, and process
  composition

Work:

- Add bounded global convergence concurrency
- Add keyed single-flight serialization per serving slot
- Claim and renew execution through existing fenced ports
- Reuse `RuntimeConvergenceSessionV1` and `plan_runtime_action_v1`
- Load the exact target through
  `PostgresRuntimeExactTargetReader::load_for_execution`
- Revalidate immutable artifact, desired-target digest, authority revision,
  authority digest, binding map, and binding fingerprint
- Run Discord bot-permission and hierarchy preflight
- Construct the exact local route
- Install it in the registry as Staged
- Never hold a gateway barrier while claiming, hydrating, calling Discord HTTP,
  or waiting for retry

Focused validation:

- Same-slot executions serialize
- Different slots respect bounded concurrency
- Claim or renewal loss removes staged local authority
- Artifact, authority, binding, or desired-target drift prevents staging
- Repeated exact claim and hydration is idempotent
- No failed hydration creates a serving route

Commit:

```text
feat(runtime): stage exact requested deployments
```

### Task B3: Replace prior routes and reconcile panels

Files:

- Add route replacement orchestration under `automation-runtime-worker`
- Modify runtime registry and gateway coordinator adapters
- Compose existing panel PostgreSQL adapters

Work:

- Implement Barrier A
- Pause admission with an opaque correlated pause token
- Move the prior route to Draining synchronously
- Resume only the exact allowed connection epoch and coordinator generation
- Keep public admission closed after resume until the exact successor ingress
  acknowledgement, bound to the new admission epoch and revision, is published
  and observed
- Stop the prior serving heartbeat outside the barrier
- Conditionally disconnect the exact prior serving identity
- Wait for active interactions to drain before route removal
- Never steal a fresh foreign serving lease
- Recheck authority before activation work
- Reuse strict panel claim, journal, reconciliation, and certificate
- Remove the staged route after a terminal panel failure
- Preserve panel journal evidence across retry and restart
- Treat ambiguous external outcomes as observation-required, not retryable
  success or failure

Focused validation:

- No prior admission crosses the replacement barrier
- Active interaction guards delay removal
- Fresh foreign leases are not stolen
- Lost pause or resume acknowledgement remains closed
- Lost, stale, or indeterminate successor ingress acknowledgement remains
  closed
- Only a complete eligible panel certificate advances the deployment
- Panel failure or authority drift removes the staged route
- Ambiguous panel results resume from journal observation after restart

Commit:

```text
feat(runtime): drain routes and reconcile exact panels
```

### Task B4: Finalize V2 certification and serving monitor

Files:

- Add finalizer and serving-lane orchestration under
  `automation-runtime-worker`
- Compose execution and serving adapters in `starring-runtime`
- Extend health projection without exposing private identities

Work:

- Reserve the canonical V2 certification intent
- Prepare certification before opening Barrier B
- Freeze the relevant owner and controller renewal transitions
- Pause the exact gateway epoch
- Activate only the exact staged local route
- Resume only through the exact barrier permit
- Build route-admission attestation from the exact pause, resume, owner lease,
  route incarnation, activation sequence, and build revision
- Commit the prepared certification only after the local route transition
- Exact-observe committed, rolled-back, or indeterminate outcomes
- Start the exact serving heartbeat monitor
- Publish durable ingress-open acknowledgement
- Project Live only after every existing exact Live predicate is true
- On monitor startup failure, conditionally disconnect and drain

Focused validation:

- Certification reservation replays byte-exactly
- A failed prepare cannot open Barrier B
- Barrier B race remains closed
- Definite rollback, definite commit, and indeterminate commit diverge safely
- A committed route without a monitor does not remain Live
- Heartbeat, disconnect, owner loss, and gateway disconnect remove Live
- Cancellation during an irreversible finalizer does not abandon outcome
  observation

Commit:

```text
feat(runtime): certify and monitor live serving routes
```

### Task B5: Dispatch interactions on the canonical shard

Files:

- Modify `tools/starring-runtime/src/discord.rs`
- Extract a shard-independent dispatcher from `crates/automation-runtime/`
- Compose `PostgresRuntimeInteractionV1`
- Add a mandatory narrow production instance teardown port
- Add production instance identity generation

Work:

- Extend the existing pinned gateway driver to deliver
  `INTERACTION_CREATE`
- Do not launch a second shard
- Convert the git-pinned gateway event inside `starring-runtime` into a
  transport-neutral owned interaction envelope containing only the exact
  typed IDs, route input, locale, token secret, and bounded payload needed by
  deterministic dispatch
- Keep the token in a zeroizing, redacted secret wrapper with no Serialize,
  Clone, or raw Debug surface
- Do not pass source-incompatible git and registry Twilight event types across
  the dispatcher boundary
- Keep concrete Twilight request and responder construction at the edge
- Reuse existing shared gateway admission, route resolution, exact version
  pinning, mutation adapter, responder, and role snapshot provider
- Dispatch static routes against the current exact serving route
- Dispatch instance routes against their historical pinned RuleSet version
- Keep admission guards alive until deterministic execution completes
- Keep broad instance-store table authority out of the runtime role
- Add fixed get, claim-deleting, mark-deleted, and retry-list teardown
  functions required by the current recipe
- Add a periodic bounded teardown-retry supervisor

Focused validation:

- Static button and modal routes execute
- Historical instance routes preserve their version
- Paused, stale-lease, stale-route, and wrong-slot requests fail closed
- Pause-before, pause-during, and resume-after interaction races are bounded
- Active interaction counts release on every success and failure
- Create, join, close, restart-resume, and periodic teardown retry all use the
  narrow production capability
- The transport-neutral envelope preserves exact identity without exposing a
  raw source-specific Twilight event type
- Cross-role direct SQL and unrelated function execution fail

Commit:

```text
feat(runtime): dispatch interactions on the production shard
```

### Task B6: Staging end-to-end gate

Work:

- Create a disposable PostgreSQL database and Discord guild namespace
- Seed only the installation and credentials required by the real product path
- Run OAuth and fresh Author authority
- Submit one deterministic authoring fixture through the trusted writer
- Promote, approve, and Apply through the real HTTP API
- Observe RuntimePending
- Start the exact release candidate runtime
- Observe exact target hydration, panel certificate, attestation, heartbeat,
  and Live
- Execute button, modal, role, channel, permission, panel, and ephemeral paths
- Restart runtime at representative durable checkpoints
- Confirm route and historical instance reconstruction
- Drain and remove all test resources

Milestone B acceptance:

- The current recipe works through the real product API and runtime
- No manual activation or smoke-only authority is used
- No customer guild or production credential is used
- The final status is truthful before, during, and after Live

Commit:

```text
test(runtime): prove requested to live staging execution
```

Milestone B completes the accepted **3–5 focused workday** staging range.

## Phase C: commercial safety

### Task C1: Add durable interaction receipts

Files:

- Add pure receipt and claim contracts in the runtime domain boundary
- Extend `automation-runtime-interaction-postgres`
- Add an ordered migration, readiness contract, and privilege tests
- Integrate the receipt into the production dispatcher
- Add a dedicated runtime interaction-token envelope key to Keychain and
  launchd provisioning

Work:

- Key a receipt by exact Discord application ID and interaction ID
- Bind tenant, installation, slot, route, RuleSet version, instance route,
  action-plan digest, and request digest
- Define closed states for claimed, deferred, executing, completed, failed,
  recovery-required, and terminal duplicate
- Claim before any Discord HTTP acknowledgement, defer, response, follow-up,
  or mutation
- Keep the database-claim deadline inside the Discord acknowledgement budget
- Encrypt a restart-recoverable interaction token with a dedicated
  domain-separated runtime envelope key
- Bind ciphertext AAD to the exact receipt, application, interaction, tenant,
  installation, route, and expiry identities
- Store ciphertext in a separate short-lived secret row so the durable receipt
  history remains immutable
- Retain that secret row only until bounded Discord token expiry and delete it
  through one narrow function at terminal completion or expiry
- If the token is missing, expired, corrupt, or unavailable after restart,
  continue safe mutation reconciliation but terminalize response recovery
  without inventing a successful user response
- Return an internal closed duplicate disposition to the dispatcher
- A completed duplicate performs zero acknowledgement, defer, response,
  follow-up, or mutation calls
- An in-flight duplicate that does not own the exclusive receipt claim
  performs zero external calls
- Only an unacknowledged receipt with a valid unexpired token and the exclusive
  current claim may send its first Discord response
- Treat same interaction identity with different semantic input as corruption
- Fence claims by process and serving route identity
- Recover expired in-flight claims through exact observation

Focused validation:

- Concurrent duplicate deliveries produce one executor
- Completed duplicate delivery produces no new mutation
- Completed and non-owning in-flight duplicates produce no Discord HTTP call
- Same identity and different request digest fails closed
- Stale process cannot complete or mutate a receipt
- Restart resumes or terminalizes the exact in-flight receipt
- Receipt database outage produces zero Discord acknowledgement, response, or
  mutation effects
- Token ciphertext, nonce, key identity, AAD, and expiry tampering fails
- Expired tokens cannot authorize a new response
- Tokens are absent from Debug, logs, errors, metrics, HTTP, and evidence
- Direct table access and unrelated function execution fail

Commit:

```text
feat(runtime): suppress duplicate Discord interactions durably
```

### Task C2: Add complete deterministic action-plan preflight

Files:

- Add a pure preflight module beside deterministic interpretation
- Extend execution planning DTOs only where required
- Add current-recipe matrix tests

Work:

- Build the complete action plan before the first Discord mutation
- Resolve every symbolic dependency, static resource reference, template,
  overwrite target, role grant, panel route, instance manifest, and channel
  relation that is knowable before execution
- Represent Discord-generated role, channel, message, and panel IDs as typed
  unresolved effect outputs and bind each one to its exact journal entry only
  after Discord returns it
- Classify every action input as statically preflightable, dependent on a
  prior journaled effect, or dependent on fresh external observation
- Validate Discord permission, hierarchy, length, count, and ordering
  requirements against one bounded fresh snapshot
- Bind the preflight certificate to the exact route, interaction receipt,
  plan digest, bindings, authority, and snapshot identity
- Reject any later execution whose identity no longer matches preflight
- Keep external-state races classified for reconciliation rather than claiming
  distributed atomicity

Focused validation:

- Every deterministic late failure in the current recipe moves to preflight
- Cross-action references are complete before execution
- A generated external ID is never guessed or claimed to be statically
  preflighted
- Later actions consume only the exact typed ID from the successful prior
  journal entry
- Snapshot or authority drift invalidates the certificate
- A malformed plan performs zero external effects
- The same plan and snapshot produce the same certificate
- The model cannot influence preflight authority

Commit:

```text
feat(runtime): preflight complete Discord action plans
```

### Task C3: Add effect journal, reconciliation, and bounded compensation

Files:

- Add pure effect-state and recovery contracts
- Extend the narrow runtime interaction PostgreSQL capability
- Add ordered migrations, readiness, and security tests
- Integrate effect recording and recovery into deterministic execution
- Add a periodic bounded recovery supervisor

Work:

- Persist a deterministic effect entry before and after each external action
- Bind action index, action identity, input, expected effect, observed external
  identity, and outcome
- Separate known success, known failure, and indeterminate result
- Never blindly replay an indeterminate create, grant, permission, or message
  action
- Define an action-by-action observation matrix for every current-recipe
  Discord effect
- Attach a durable per-action correlation marker or audit-log reason where
  Discord supports an independently observable exact identity
- Observe Discord and reconcile the expected effect first
- Adopt an observed effect only when the action-specific matrix proves one
  unique exact correlation identity
- Never adopt a role, channel, message, overwrite, or panel merely because its
  name or mutable attributes match
- If Discord exposes no unique correlation evidence for an indeterminate
  create, keep the effect `recovery_required`; do not replay or guess
- Classify a conflicting effect as recovery-required
- Compensate safely reversible effects in reverse dependency order
- Preserve and verify preimages where removal or permission restoration is
  required
- Execute non-compensable response actions only after mutable provisioning
  succeeds
- Stop admission for an affected route or instance when automatic recovery is
  unsafe
- Retry periodic teardown and recovery without broad database authority

Focused validation:

- Failure at every current-recipe action boundary
- Process termination before request, after request, after Discord success,
  before journal success, and during compensation
- Discord 403, 404, 429, timeout, connection loss, and malformed response
- Exact observed success is adopted without duplication
- Name-only or attribute-only matches are never adopted
- Unsupported-correlation indeterminate creates remain recovery-required
- Conflicting observed state becomes recovery-required
- Safe compensation restores the exact preimage
- Unsafe compensation never deletes an unrelated customer resource
- Restart and periodic recovery converge or remain explicitly blocked

Commit:

```text
feat(runtime): reconcile partial Discord execution effects
```

Phase C reaches the 4.5–7 focused workday hardening checkpoint. It is not a
commercial completion claim until every Phase D certification gate passes.

## Phase D: release certification

### Task D1: Complete restart and failure cohorts

Required runtime checkpoints:

- Requested
- Claimed
- PreflightReady
- ActivationApplying
- ReconcilingPanels
- AwaitingGatewayReady
- Certification reserved
- Certification commit indeterminate
- Live before first heartbeat renewal
- Live with fresh serving lease
- Draining with active interactions
- Suspended
- Recovery-required interaction
- Shutdown

Required failures:

- Database unavailable before claim
- Database unavailable before effect
- Discord unavailable before effect
- Discord outcome indeterminate
- Gateway disconnect
- Owner lease loss
- Controller lease loss
- Writer-fence change
- Installation authority rotation
- Binding-map change
- Process kill and restart
- Duplicate HTTP authoring turn
- Duplicate Discord interaction

Acceptance:

- No false Live
- No stale writer
- No duplicate mutable effect
- No missing durable recovery state
- No unsafe automatic deletion
- Every retryable state has a bounded next action
- Every terminal blocked state has a stable public code and operator procedure

### Task D2: Run the final disposable-guild product E2E

Sequence:

1. Create a unique disposable test database and Discord resource prefix
2. Confirm no prior runtime or smoke process owns the test guild
3. Start the exact candidate API, Codex worker, runtime, and tunnel
4. Authenticate through OAuth
5. Submit a one-shot Luna authoring request
6. Confirm encrypted generation and safe PreviewReady projection
7. Promote, approve, and Apply through product endpoints
8. Observe RuntimePending and then exact Live
9. Execute create-room and join interaction paths
10. Deliver a duplicate interaction and prove one external effect
11. Kill and restart runtime
12. Prove route and pinned instance reconstruction
13. Inject one indeterminate external-effect scenario and prove reconciliation
14. Apply an updated or rollback target and prove drain and replacement
15. Disconnect the gateway and prove Live loss
16. Tear down all test resources
17. Confirm no unresolved test operation, receipt, journal, route, instance,
    role, channel, or panel remains

Evidence records:

- Exact Git commit SHA
- Binary SHA-256 values
- Migration ledger
- Non-secret build revision
- Deployment, route, receipt, and attestation identities
- HTTP status and closed public codes
- Discord test resource IDs
- Restart and injected-failure checkpoints
- Cleanup result

Evidence excludes:

- OAuth or worker tokens
- Discord bot token
- Database URL or password
- Keychain material
- Key material or material digest
- Full user transcript

### Task D3: Run the final complete gate

Create the final PR, update it onto the current main base, fetch the
GitHub-generated merge candidate, and record its commit and tree identities.
Run the complete local, PostgreSQL, and disposable-guild gates on that exact
merge-candidate checkout:

```sh
cargo fmt --all -- --check
cargo build --locked --workspace --all-targets
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p interaction-smoke --features unsafe-dev-activation
npm --prefix tools/codex-worker run check
npm --prefix tools/codex-worker test
npm --prefix eval/codex-worker-slo run check
npm --prefix eval/design-harness ci
npm --prefix eval/design-harness run audit
npm --prefix eval/design-harness run check
```

The release PostgreSQL manifest is explicit and serial:

```sh
cargo test --locked -p automation-ruleset-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-instance-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-panel-installation-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-ruleset-activation-postgres -- --ignored --test-threads=1
cargo test --locked -p authoring-promotion-postgres -- --ignored --test-threads=1
cargo test --locked -p authoring-application-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-ruleset-dispatch -- --ignored --test-threads=1
cargo test --locked -p automation-ruleset-readiness -- --ignored --test-threads=1
cargo test --locked -p automation-runtime-convergence-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-runtime-execution-postgres --test postgres_security -- --ignored --test-threads=1
cargo test --locked -p automation-runtime-serving-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-runtime-interaction-postgres -- --ignored --test-threads=1
cargo test --locked -p automation-runtime-panel-postgres -- --ignored --test-threads=1
```

The authoring writer suite belongs to
`authoring-application-postgres`. Interaction receipts, encrypted token
storage, effect journals, and recovery belong to
`automation-runtime-interaction-postgres`. If implementation creates a new
standalone PostgreSQL test target, add its exact command to this manifest.

Update `.github/workflows/ci.yml` so every command in this manifest is required
in the PostgreSQL job. Do not rely on an undocumented wildcard or prose claim
that every integration test ran.

Merge only while the PR base is unchanged. After merge:

- Record the resulting main merge commit and tree identities
- Require the merged tree identity to equal the certified merge-candidate tree
- Rerun merge-candidate certification if the tree differs
- Require `checks` and `postgres` push CI on the resulting main merge commit

The non-CI Phase D evidence applies to the certified tree. Push CI applies to
the exact main merge commit that owns that identical tree.

### Task D4: Update source-of-truth and operations

Files:

- `CURRENT_STATE.md`
- Starring API staging and cutover runbooks
- Starring runtime staging and cutover runbooks
- Codex worker operations runbook
- Relevant handoff document

Work:

- Correct crate, tool, migration, role, pool, and Keychain inventory counts
- Replace empty-open-only wording only after the exact Live E2E passes
- Document authoring degraded behavior
- Document runtime recovery-required behavior
- Document duplicate receipt and effect-journal inspection using redacted
  projections
- Document rollback, shutdown, backup, restore, and failure drills
- Keep unsupported recipes and remaining limitations explicit

Commit:

```text
docs(operations): certify commercial authoring and runtime path
```

Phase D completes the accepted **5–8 focused workday total** only after its
exact merged-main CI and evidence requirements are satisfied.

## Validation cadence

### Every functional commit

- `cargo fmt --all -- --check`
- `git diff --check`
- Tests for directly changed packages
- Direct dependency and source guards

Do not run the complete workspace, all PostgreSQL suites, GitHub Actions, or a
live Discord cohort for every commit.

### Migration commits

- Static migration guard
- Fresh and upgrade migration tests
- Only the affected real PostgreSQL adapter suite
- Positive and negative capability tests
- Serial execution where shared PostgreSQL fixtures require it

### Milestone A

- Affected authoring packages
- Complete `authoring-application-postgres` ignored suite
- Live Luna one-shot and multi-turn samples
- Conversation-to-promotion vertical slice

### Milestone B

- Affected runtime packages
- Five existing runtime PostgreSQL adapters
- Fake Discord lifecycle and race cohorts
- Disposable-guild Requested-to-Live E2E

### Final release candidate

- Complete workspace, Clippy, formatting, Node, evaluator, PostgreSQL, restart,
  failure-injection, and disposable-guild gates
- One final PR
- Both GitHub `checks` and `postgres` jobs green
- Record the feature head and GitHub-generated merge-candidate commit and tree
  identities
- If the base branch changes after merge-candidate validation, regenerate and
  revalidate the candidate
- Preserve the functional commits through the repository's reviewed merge
  method
- Require the resulting main merge tree to equal the certified candidate tree
- Record the resulting main merge commit and require its push CI to be green

## Functional commit order

1. `refactor(harness): extract reusable Codex worker client`
2. `feat(authoring): add trusted conversation application`
3. `feat(postgres): add encrypted authoring generation writer`
4. `ops(postgres): provision trusted authoring writer capability`
5. `feat(api): expose trusted conversational authoring`
6. `test(authoring): prove conversation to promotion vertical slice`
7. `feat(runtime): extend production lifecycle for route serving`
8. `feat(runtime): stage exact requested deployments`
9. `feat(runtime): drain routes and reconcile exact panels`
10. `feat(runtime): certify and monitor live serving routes`
11. `feat(runtime): dispatch interactions on the production shard`
12. `test(runtime): prove requested to live staging execution`
13. `feat(runtime): suppress duplicate Discord interactions durably`
14. `feat(runtime): preflight complete Discord action plans`
15. `feat(runtime): reconcile partial Discord execution effects`
16. `docs(operations): certify commercial authoring and runtime path`

Do not squash these functional boundaries. Corrective commits may be added
when evidence discovers a distinct defect. Do not rewrite unrelated user
changes.

## Definition of done

The backend and internal core are a commercial-safe candidate only when all of
the following are true:

- One-shot and conversational authoring use one authenticated product API
- The fixed Luna-medium worker is the only active authoring provider
- A committed exact replay and a concurrent same-process duplicate never spend
  another model call
- Only validated and simulated encrypted generations are promotable
- Promotion, approval, and Apply remain separate human-authorized boundaries
- A real product Apply reaches Requested without manual activation
- `starring-runtime` advances the exact deployment to Live
- Live requires exact attestation, route, owner, heartbeat, serving lease, and
  ingress evidence
- The canonical Discord shard executes the current recipe
- Historical instances retain the RuleSet version they were created with
- Duplicate Discord delivery produces one mutable external effect
- Complete deterministic preflight runs before the first mutable effect
- Partial external failure is durably reconciled, safely compensated, or
  explicitly blocked
- Restart at every durable checkpoint has a tested reconstruction outcome
- Database roles have only fixed capability functions and no relation DML
- Secrets are absent from source, commits, logs, HTTP, and evidence
- No event-time or deployment-time AI path exists
- Final complete local gates, PostgreSQL gates, and disposable-guild E2E are
  green on the exact merge-candidate tree
- The merged-main tree is byte-identical to that certified tree and GitHub
  push CI is green on the exact main merge commit

This definition certifies the current private-study-room recipe. It does not
claim arbitrary Discord automation or game support.

## Rollback rules

- Authoring rollout can be disabled independently while preserving promotion,
  decision, Apply, and status APIs
- A failed authoring turn never advances the generation head
- A failed runtime target never reuses a stale route as the new exact target
- An unsafe or indeterminate runtime finalizer keeps admission paused
- A new route is removed if it never receives a complete panel certificate
- A Live route losing exact serving evidence is disconnected and drained
- A commercial-hardening migration must be additive until its rollback and
  restored-cluster behavior is proven
- A release rollback uses the existing approved exact prior target and the
  same runtime convergence path
- No operator command directly edits an active pointer, authoring generation,
  runtime attestation, receipt, or effect journal

## Async authoring queue trigger

The synchronous authoring V1 remains valid while:

- Worker concurrency is intentionally small
- Request duration stays inside the bounded authoring timeout
- Retry after a pre-commit process crash is acceptable
- Queue depth and sustained concurrency remain low
- One Mac mini remains the deployment target

A durable async queue becomes a separate accepted design when any of these are
measured:

- Material client disconnect loss
- Sustained saturation
- More than one worker process
- High-availability authoring requirement
- Queue wait becomes part of a public SLO
- Model execution duration no longer fits a bounded HTTP request

The async design must encrypt queued human input, use a durable claim lease,
preserve exact idempotency, and keep the same generation CAS and authority
recheck. It must not weaken this plan's trusted-writer boundary.

## Normative references

- [Current State](../../../CURRENT_STATE.md)
- [Luna Commercial SLO Program](../specs/2026-07-17-luna-commercial-slo-program-design.md)
- [Authoring Promotion Bridge](../specs/2026-07-18-authoring-promotion-bridge-design.md)
- [Production Control API Runtime Convergence](../specs/2026-07-19-production-control-api-runtime-convergence-design.md)
- [Production Runtime Worker Composition](../specs/2026-07-22-production-runtime-worker-composition-design.md)
- [macOS Codex Worker Operations](../runbooks/2026-07-17-macos-codex-worker-operations.md)
- [Production Control Plane Cutover](../runbooks/2026-07-19-production-control-plane-cutover.md)
- [macOS Starring Integrated Staging Cutover](../runbooks/2026-07-29-macos-starring-integrated-staging-cutover.md)
- [macOS Starring Runtime Staging Operations](../runbooks/2026-07-29-macos-starring-runtime-staging-operations.md)

The accepted safety contracts in the referenced specifications remain
authoritative. This plan fixes the remaining implementation order, estimate,
commit boundaries, validation cadence, and completion criteria; it does not
silently amend an accepted safety contract.

Where an older status statement claims that empty-open is production serving,
omits the trusted writer, omits duplicate interaction receipts, or omits
partial-effect recovery, this plan and the later verified source-of-truth
update take precedence for implementation status only.
