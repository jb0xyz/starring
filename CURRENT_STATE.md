# Starring — Current State

## Purpose of This Document

This is the source of truth for Starring's current implementation state and
crate structure. Older design documents (the OCI architecture note, the original
repo-structure map, early DesiredState-centric descriptions) provide historical
context, but where they conflict with this document, **this document wins**.
Detailed rationale lives in the per-topic specs, plans, and runbooks under
`docs/superpowers/` — this file is the map, not the territory.

## System Summary

Starring is an AI-based Discord control plane: "Terraform / Kubernetes for
Discord with a natural-language frontend." A Rust workspace of **38 crates and 6
tools**, PostgreSQL-backed at its durable runtime boundaries, organized into
three layers. The defining safety principle is constant across all layers:

> AI designs at install/authoring time. The runtime executes deterministically.
> Event-time LLM calls are forbidden (enforced per-crate by `no_ai_gateway`
> tests).

Layer 1 and Layer 2 production runtime boundaries are PostgreSQL-or-die and
fail-closed: they do not start, and do not mutate Discord, on an unsafe or
unverifiable state. The model-facing Layer 3 harness keeps local authoring state
in SQLite and cannot touch Discord or the production database. A separate
server-side promotion boundary bridges sealed authoring artifacts into the
PostgreSQL-backed Layer 2 lifecycle without exposing that authority to the model.

## Architecture at a Glance

```
Layer 1 — DesiredState Control Plane (one-shot guild configuration)
  natural language ─▶ DesiredState ─▶ diff ─▶ policy ─▶ preview ─▶ approval ─▶ execute

Layer 2 — Durable Interaction Rule Plane (dynamic, per-interaction automation)
  RuleSet authoring ─▶ publish (immutable version) ─▶ approval-bound activation
    ─▶ readiness-gated hydration ─▶ pinned per-interaction dispatch ─▶ teardown

Layer 3 — Conversational Intent Authoring (design-time only)
  conversation ─▶ bounded Intent IR ─▶ deterministic Recipe Compiler
    ─▶ atomic Draft candidate ─▶ validate ─▶ simulate ─▶ preview
```

Layer 2 remains the live-proven engine arc. The current development checkpoint
connects a sealed Layer 3 design to Layer 2 publication and product-bound
activation without exposing those capabilities to the model. Event-time
execution remains deterministic. All layers share the Discord domain model and
the resource-binding layer.

## Layer 1: DesiredState Control Plane

The one-shot plane: it takes a desired guild configuration (authored or produced
from natural language by the AI gateway), diffs it against current state, runs it
through policy and a preview, gates it on human approval, and executes it. This
is the plane that turns "make the server look like X" into a set of Discord
mutations, once. Its pipeline crates exist (`ai-gateway`, `desired-state`,
`desired-compiler`, `diff-engine`, `operation-graph`, `policy-engine`,
`preview`, `approval-manager`, `executor-core`, `simulator`, `virtual-apply`).
Live end-to-end maturity of this plane is not the subject of this document's
verification claims; the certified, live-proven arc below is Layer 2.

## Layer 2: Durable Interaction Rule Plane

The dynamic plane, and the focus of the 16–18f arc. Server operators define an
`InteractionRuleSet` —
panels, buttons, modals, and rules that react to interactions (a "Create study
room" button, a "join" button, a "close" button). A ruleset is published as an
immutable, content-addressed, versioned artifact, activated behind an approval
boundary, hydrated at boot behind a readiness gate, and thereafter each
interaction is dispatched deterministically against the exact ruleset version its
instance was born with. Rooms (instances) own their complete Discord footprint
and can be torn down durably. This plane has been proven live end-to-end,
including RuleSet rollback and approval-bound activation, and is guarded by CI.

## Layer 3: Conversational Intent Authoring

The design-time authoring layer supports complete one-shot requests and bounded
multi-turn conversation for the supported recipe: one hub-channel decision or
a discussion followed by an explicit build turn. Intent protocol V4 uses one
bounded Luna-medium Core extraction for default, discussion, fallback, gap, and
rejection paths. The active Core wire has no model-authored objective or safety,
runtime, and recipe-detail authority. Deterministic current-human grounding owns
request mode, direct-preview intent, live-mutation and gate-bypass safety roles,
secret-disclosure roles, locale, room-close authorization, closed runtime
requirements, mandatory-control restatements, and selected detail facets before
adjudication. A successful
private-study-room request with explicit copy, naming, or controls customization
uses exactly one additional isolated detail extraction, for two model calls and
two model tool calls total. For the `starring.private_study_room@1` capability,
deterministic Rust code owns capability and safety adjudication, recipe
selection, normalization, defaults, recipe versioning, generated keys,
permission policy, references, action order, instance manifests, validation,
simulation, and preview. The model never assembles the low-level RuleSet for
this path.

V4 separates provenance and semantic identity. An ordered request-evidence
chain binds accepted human turns; a closed route-semantic digest is distinct
from the evidence-bound adjudication digest; compiler-input identity remains
distinct from revision-independent semantic-intent identity; and compiled-plan,
candidate-RuleSet, candidate-Draft, stage, and complete-transcript bindings
detect inconsistent local state. Model-authored discussion presentation is
bounded and non-authoritative. V3 Intent snapshots and branch-local pre-release
V4 snapshots are rejected rather than re-signed under the current protocol.
These SHA-256 bindings detect corruption and inconsistent partial edits; they
are not authentication against an attacker who can rewrite the SQLite snapshot
and all of its digests.

The active detail router exposes only the selected `copy`, `naming`, and
`controls` roots. Its flat serving wire and parser share the exact dynamically
derived schema instance. The isolated second request sees only the current
human turn and harness-owned detail state. A path-only ticket is derived before
the call, and every accepted custom scalar or pattern affix must match the
independently grounded slot-specific exact literal in that current turn before
compilation. Duplicate keys, cross-slot substitutions, stale-turn literals, and
path or value drift fail closed without repair or Draft mutation.

The authoring surface currently exists as the pure `design-harness` crate and
the SQLite/loopback-worker `design-harness-cli` edge. It is a CLI and evaluation
checkpoint, not a production API or UI. The only implemented product recipe is
the private study room recipe. Typed-planner fallback is classified but not yet
handed off to an actual typed-planner session.

The implemented `authoring-application` route authenticates an opaque product
session, derives a fresh Discord guild-authority observation, and loads one
atomic, revalidated `PreviewReadyArtifactV1` generation through server-owned
ports before passing it to the pure `authoring-promotion` workflow. That
workflow publishes its exact
`InteractionRuleSet` as an inactive immutable version, creates a
`ProductAuthoring` activation request, and durably binds that request to the
exact promotion journal. The lower-level workflow remains a public core API and
depends on production composition to keep direct transport access unreachable.
The
`authoring-promotion-postgres` adapter persists the monotonic workflow. A
database trigger rejects a product activation link unless the exact
`ActivationPending` promotion row, request, target, requester, policy, payload,
and approval-context identities agree. Approval reuses the product-bound
decision evidence, but ProductAuthoring claim, apply, and resume are rejected by
the generic activation store. Product Apply is allowed only through the
authenticated atomic product-control boundary; publication and promotion never
change the active pointer.

The pure `authoring-application` boundary accepts only bounded product commands
and opaque credentials. It sequences authentication, CSRF verification for
mutations, fresh Discord authority, exact tenant and installation scope,
payload-bound approval, atomic Apply, and deployment-status projection. Durable
PostgreSQL adapters now persist OAuth flow digests, opaque session and CSRF
digests, encrypted authoring generations, current and historical installation
authority, approval receipts, audit evidence, atomic Apply outcomes, and exact
runtime deployment selectors. The PostgreSQL installation-authority source now
binds the exact authenticated principal and session fingerprint to one active
tenant and installation plus its exact current authority revision and digest in
a bounded read-only repeatable-read snapshot. It commits before the existing
Discord adapter performs live guild authorization, and normal inaccessible
states are non-enumerating while durable graph corruption is redacted and fails
closed. Product-session authentication now reads session-only and mutation
projections through separate versioned, fixed-search-path database functions.
The mutation projection returns a session-bound comparison tag instead of the
stored CSRF digest. A separate compare-and-set touch function rechecks database
time, expiry, revocation, observed timestamps, the idle bound, and the touch
interval before extending an active session. These authentication read and touch
operations are independently usable by a non-owner role with only the exact
function execution privileges. Apply writes the guarded active pointer,
`Approved -> Applying -> Applied`, Requested deployment, receipt, aliases, and
audit evidence in one serializable transaction. Normal baseline, binding, or
policy drift instead commits a durable `Superseded` terminal record. Immutable
target corruption remains a distinct bounded error and does not get hidden as
drift.

Additive transport-neutral observation ports preserve exact promotion identity,
revision, immutable target, approval digest and expiry, decision database time,
runtime database time, Live heartbeat, and serving-lease evidence without adding
a second database read. Promotion replay is bound to its deterministic identity
and exact-replay disposition; new submission is bound to the full planned
request digest. The stable promotion milestone is named `ActivationLinked`
rather than implying the linked activation is still awaiting approval. Runtime
failure metadata crosses the new observation boundary only through a closed
public-code enum, while the older string-bearing public projections remain
source compatible. The V1 HTTP wire retains its serving-lease expiry field for
compatibility. V2 exposes bounded timestamps only for exact disconnected,
expired, or fresh serving evidence.

The additive deployment operational-status V2 boundary preserves the V1 SQL
signature, result and evidence shapes, HTTP wire response, and grant surface
while exposing bounded convergence detail through a separate capability. The
internal V1 runtime reason precedence is hardened so exact-identity mismatch is
classified before disconnect or expiry and mismatched timestamps cannot affect
the reason code; its HTTP projection remains Pending. An owner-only SQL core
reads the original status evidence plus deployment, failure, and attestation
attempt numbers in one query; V1 and V2 are projections over that core. A
separate V2 reader readiness contract permits only its dedicated
database-identity and V2 status functions and rejects V1, core, every user
relation or sequence capability, DML, DDL, and unrelated executable access.
The pure application reports closed convergence phases, retry waiting or due state,
blocked recovery or authority-restoration action, exact attestation attempt, and
seven closed serving-freshness states. Wrong identity evidence is redacted, and
controller IDs, fencing tokens, failure IDs and messages, process identities,
build revisions, shard IDs, and panel digests never cross the product boundary.
The hardened HTTP library exposes this only through the separate
`ProductControlOperationalFacadeV2` router at
`GET /v2/installations/{installation_id}/promotions/{promotion_id}/deployment`;
the existing router continues to return 404 for that path.

A provisioned product installation exclusively owns its `(guild, RuleSet key)`
slot. Legacy and generic direct activation attempts plus installation takeover
share one transaction-scoped slot lock and recheck ownership after waiting.
Product Apply retains its separate product-lane lock and atomic transaction. A
product activation cannot
remain `Applying` at commit, an `Applied` product activation record is immutable,
and the active pointer must identify the exact latest deployment lineage for
that installation. Database constraints and Rust store guards both reject
legacy or generic activation of a product-owned slot.

The runtime convergence core and PostgreSQL adapter implement fenced claim,
drain, activation, panel reconciliation, exact attestation, serving leases,
heartbeat, disconnect, stale-Live recovery, and strict status projection. A
deployment retains its historical Apply authority while every runtime mutation
checks the current lifecycle and exact binding identity. Policy-only authority
rotation keeps Live; a changed binding map, including one paired with a spoofed
unchanged fingerprint, fails closed. Product status reports Live only for the
exact desired-target digest, attestation, process generation, connected serving
lease, and unexpired heartbeat.

`product-control-http` provides the hardened Axum route contract for OAuth,
session, promotion, approval, rejection, Apply, product status, deployment
status, and health. It enforces exact Host and Origin, strict JSON, bounded
bodies and concurrency, deadlines, panic isolation, double-submit CSRF,
host-only Secure cookies, idempotency-key validation, no-store responses, and
closed response validation. OAuth start has a process-wide bounded admission
budget, and installation-scoped authority failures conceal whether a guessed
installation exists. The session-bound
`GET /v1/installations/{installation_id}/authority-check` route performs the
same fresh `Apply` authority evaluation used by product mutation admission and
returns only an empty `204`; it does not retain authority evidence or mutate
Discord, installation, promotion, or runtime state. A staging-only, fail-closed operator SQL
workflow can atomically create a tenant, installation, immutable authority
revision 1, and its initial runtime writer fence. It supports exact replay,
requires the canonical empty-binding fingerprint, and does not create a
permanent provisioner role. A public installation administration API remains
deferred. `tools/starring-api` now supplies the closed
production-facade bridge, validated environment and Keychain configuration,
thirteen independently credentialed PostgreSQL pools, aggregate and post-bind
deep readiness, and a bounded loopback-only HTTP process. Database connection
material is complete and explicit; ambient PostgreSQL variables and `.pgpass`
cannot fill missing authority or transport fields. Runtime readiness is
supervised outside the request path; its first error, timeout, or panic closes
business admission and the listener before graceful drain. Explicit parameter
privileges and per-role database settings are treated as excess capability, so
server-side privilege drift also closes admission. The checked-in
LaunchAgent and exact thirteen-role bootstrap are deliberately staging-only.
That process is a runnable staging control plane; it is not evidence of
production Live because the separate runtime worker, trusted authoring writer,
isolated production secret account, and retention scheduler remain absent.

The active authoring provider is `codex_chatgpt`, pinned to
`gpt-5.6-luna` with `medium` reasoning effort and ChatGPT authentication. The
CLI reaches it only through the bearer-authenticated worker bound to
`127.0.0.1:18181`; that raw worker is not a Cloudflare origin. The retired
`local.cloudflared.starring`, `local.llm-api`, and `local.ollama.server`
services are disabled and unloaded. The `gemma4:12b-mlx` model file remains on
disk only as rollback material. Interactive CLI startup now defaults to Intent
Recipe mode and fails closed before network access when its bindings are absent.
Adaptive and Typed Plan remain explicit legacy rollback modes only.

The worker health contract exposes its stable process instance, source digest,
capacity, timeout, and monotonic accepted and settled completion counters. The
Luna V4 matrix requires a dedicated one-active, zero-queue worker and proves
that every phase counter delta equals that phase's reported model calls; another
valid completion request invalidates the cohort instead of contaminating it.
The current 2026-07-17 clean-source matrix at
`7f138b308644f954cd38ceee78768f3d6b7bf551` passed all 232/232 Promptfoo rows
and executed the exact 298 model/tool calls with zero provider errors, repair
attempts, or retries. Every request, route, and adjudication identity class was
stable. The certified catalog identity is extractor revision 16, normalizer
revision 15, and registry digest
`fc66223bee4c1ec2e3dd2535a4a4ad1dae6a17f3b896b1a29a6998cde4d8535c`.
The earlier normalizer-12 certificate remains immutable historical evidence;
two interrupted normalizer-13 and normalizer-14 runs are diagnostic only and
are not pooled into this result.
This certifies the bounded single-worker authoring cohort, not commercial
concurrency, soak behavior, high availability, or live Discord execution.

The current Luna V4 evidence and continuation state are recorded in
`docs/superpowers/handoffs/2026-07-17-luna-v4-acceptance-hardening-handoff.md`.
The original V4 implementation internals remain recorded in
`docs/superpowers/handoffs/2026-07-15-intent-v4-semantic-identity-handoff.md`;
its serving and continuation instructions are historical.
The Gemma V3 cohort handoff remains the historical live baseline, and the
2026-07-14 handoff remains background for the original Intent IR, Recipe
Compiler, persistence, runtime, and server checkpoint.

## Workspace Topology

38 crates, 6 Rust tools. The recurring pattern is **pure core + edge adapter**: pure
crates hold the domain and logic and are forbidden `sqlx`/`twilight`
dependencies (guarded by `dependency_guard` tests); a paired `*-postgres`
adapter (or the `automation-runtime` Twilight edge) provides persistence and
Discord I/O.

- **Shared domain**: `discord-model`, `domain`, `resource-resolution`,
  `desired-state`.
- **Layer 1 pipeline**: `ai-gateway`, `desired-compiler`, `diff-engine`,
  `operation-graph`, `policy-engine`, `preview`, `approval-manager`,
  `executor-core`, `simulator`, `virtual-apply`, `bot-runtime`.
- **Layer 2 rule engine**: `automation-state` (schema), `automation-core`
  (interpret/run/validate), `automation-runtime` (Twilight edge).
- **Layer 3 authoring**: `design-harness` (pure Draft, conversation, Intent IR,
  Recipe Compiler, candidate gates, and exact simulation).
- **Layer 3 to Layer 2 promotion and product control**:
  `authoring-application` (pure authenticated use cases),
  `authoring-application-discord` (Discord OAuth and fresh guild authority),
  `authoring-application-postgres` (identity, encrypted snapshots, decisions,
  Apply, and status), `product-control-http` (hardened transport contract),
  `authoring-promotion` (workflow and product activation bridge), and
  `authoring-promotion-postgres` (durable journal).
- **Runtime convergence**: `automation-runtime-convergence` (fenced state
  machine) and `automation-runtime-convergence-postgres` (durable claims,
  attestations, serving leases, recovery, and status).
- **Layer 2 durable ruleset**: `automation-ruleset` (registry core),
  `automation-ruleset-postgres`, `automation-ruleset-readiness` (hydration +
  activation gate), `automation-ruleset-dispatch` (pinned dispatch),
  `automation-ruleset-activation` + `-postgres` (approval-bound activation
  authority).
- **Layer 2 durable instances**: `automation-instance` + `-postgres`,
  `automation-instance-teardown`, `automation-panel-installation` + `-postgres`.
- **Rust tools**: `interaction-smoke` (feature-gated, test-database-only Layer 2
  manual runner),
  `executor-smoke`, `starring-demo`, `ai-eval`, `design-harness`
  (Luna-medium/SQLite CLI and evaluation edge), and `starring-api`
  (authenticated loopback product-control process). `codex-worker` is a
  separate private loopback ChatGPT-login Codex service rather than a workspace
  member.

Persistence is forty-two migrations under `/migrations`, including the
original instance and RuleSet stores, product-bound activation context and
terminal states, the authoring promotion journal, atomic Product Apply and
runtime deployment, runtime convergence, current-versus-historical binding
separation, artifact integrity, exclusive product-slot ownership, scoped
installation-authority reads, and scoped product-session authentication.

## Durable RuleSet Lifecycle

```
publish   immutable, content-addressed, monotonic version in PostgreSQL (dedup by content hash)
activate  approval-bound (see safety boundary); readiness re-verified at apply
hydrate   at boot, the active artifact is re-validated (schema, structural, hash, binding,
          policy, capability) and only then promoted to the running ruleset — fail-closed
dispatch  each interaction runs against its instance's pinned version, re-evaluating readiness
          against a fresh Discord snapshot per click; no active-version fallback
rollback  re-activating a prior version is a durable pointer swap; existing instances keep their
          pinned version, new instances pin the newly active one
```

Proven live: publish v1/v2, activate, roll back, two restarts recovering the
active pointer + instance pins + panel state from PostgreSQL.

## Durable Instance Lifecycle

```
create    identity preallocated before any Discord mutation; the rule creates role/channel/panels
register  a complete, immutable resource footprint is persisted atomically (created == owned)
dispatch  join/close/etc. run against the instance's pinned ruleset version
teardown  Active → Deleting → Deleted; delete messages → channels → roles idempotently;
          NotFound/Forbidden/RateLimited distinguished; resumable after restart; shared
          bindings never deleted
```

## Activation and Approval Safety Boundary

Legacy and manual RuleSet pointers change only through the activation authority
(`automation-ruleset-activation`). No normal CLI, API, or runtime path calls its
low-level activation (`activate_if_ready` / `RuleSetStore::activate`) directly;
guard tests enforce the allowlist. A legacy activation is a durable request
bound to an immutable target (guild, key, version, content hash); it requires a
quorum of **distinct** approver identities supplied by its trusted caller (the
requester cannot self-approve). Its leased apply service re-verifies the target
and runs a **fresh** readiness check before mutating the pointer, while
per-request and per-`(guild, key)` CAS converge crashes and concurrent
execution. A development-only `unsafe-dev-activate` escape is
compile-feature-gated, absent from normal builds, and cannot target a
product-owned slot.

Product-authored requests begin `Unlinked`. Both the pure bridge and a
PostgreSQL trigger require an exact `ActivationPending` promotion journal before
the request becomes approvable. Approval binds the exact promotion payload,
resource projection, policy, and observed active baseline. Apply reloads the
fresh environment and rechecks binding revision, fingerprint, baseline, and
readiness even when the requested target is already active. Product-authored
pointers change only through this authenticated, serializable, atomic Product
Apply boundary; generic activation claim, apply, resume, and direct store paths
reject them.

Once an installation claims a RuleSet slot, every committed active pointer for
that slot must be backed by its exact latest product deployment and linked,
`Applied` activation request. Legacy activation and installation takeover
serialize on the same slot lock and recheck after waiting, so both race orders
fail closed. Direct SQL cannot leave a product activation in `Applying`, mutate
an `Applied` record, delete or retarget the pointer, or install over an
in-flight legacy activation.

## Core Safety Invariants

- Layer 1 and Layer 2 production runtime boundaries are PostgreSQL-or-die and
  fail-closed: no start and no Discord mutation on an unsafe or unverifiable
  state. The model-facing Layer 3 harness remains local and pure. The intended
  production composition exposes publication and approval-request creation only
  through the separate server-authorized promotion boundary, and neither
  operation activates a RuleSet.
- AI at design/authoring time only; runtime is deterministic; event-time LLM is
  forbidden (`no_ai_gateway` per crate).
- Pure crates carry no `sqlx`/`twilight` regular dependency (`dependency_guard`).
- One readiness gate shared by boot hydration and activation, so the read side
  and write side cannot diverge.
- Interactions dispatch against the instance's pinned version, re-checking
  readiness against a fresh snapshot per click.
- Legacy/manual pointers mutate only through the approval-bound activation
  authority; product-owned pointers mutate only through authenticated atomic
  Product Apply.
- Product Apply commits pointer, decision, Requested deployment, receipt, and
  audit evidence atomically, or commits none of them. Lost outcomes are resolved
  only by replaying the same idempotency key.
- A product installation exclusively owns its RuleSet slot. Every active
  pointer has exact latest-deployment lineage, legacy or generic direct
  activation and takeover serialize on one slot-lock namespace, Product Apply
  serializes on its product lane, and no product `Applying` residue or mutable
  `Applied` evidence can commit.
- Product Live requires exact current target, historical Apply identity, current
  binding identity, immutable attestation, connected serving ownership, and an
  unexpired heartbeat. Policy-only authority changes do not invalidate the
  binding; binding content drift does.
- The model-facing harness has no product identity, approval, Apply, deployment,
  Discord, or PostgreSQL tool and cannot cross the production control boundary.

## Verification and CI

- **CI** (`.github/workflows/ci.yml`, GitHub Actions, push + PR): a DB-less job
  (fmt, build, `cargo test --workspace`, clippy `-D warnings`, unsafe-dev feature
  build, and design-harness JavaScript/Promptfoo static checks) and a PostgreSQL
  job (nine adapter or integration packages' ignored tests, serial). No live Discord
  or LLM in CI.
- **Test volume**: the complete Rust workspace suite, the design-harness
  JavaScript evaluator and acceptance self-tests, two Promptfoo configuration
  validations, and the ignored PostgreSQL integration suites for contention,
  CAS, lease, partial-unique, and reconnect behavior. At the final Luna V4
  checkpoint, the focused `design-harness` library target passed 742 tests, the
  CLI gate passed 82 tests plus its dependency guard, and the JavaScript gate
  passed 106 tests. Relevant clippy `-D warnings` and formatting gates also
  passed. The exact resumable repeated Luna matrix is implemented, statically
  verified, and passed live at 232/232 samples and 298/298 planned model calls.
  The clean evidence source also passed GitHub Actions CI run 31: the complete
  workspace checks and PostgreSQL integration job were both green. CI remains
  separate from the local live-model certificate.
- **Live certification**: Layer 2 manual runbooks (real bot, guild, PostgreSQL)
  prove its end-to-end lifecycle; they are never wired into CI. The Luna V4
  authoring cohort did not run a live Discord integration.
- **Current product-slot checkpoint**: the exact clean-database PostgreSQL CI
  sequence passed all nine packages and 209 tests. Focused product Apply passed
  34/34, including both advisory-lock race orders and final-pointer transaction
  semantics. The isolated `interaction-smoke` suite passed 24/24.

## What Is Complete

Stated as capabilities (durable across the phase numbering):

- Immutable, content-addressed, monotonically versioned RuleSet registry.
- PostgreSQL-backed active pointer and durable instances.
- Readiness-gated boot hydration (fail-closed).
- Instance version pinning at creation.
- Pinned-version, per-interaction dispatch with per-click fresh readiness.
- Safe RuleSet activation sharing the hydration gate.
- Durable panel installation and reconciliation across versions.
- Preallocated instance identity with a complete, owned resource footprint.
- Resumable, idempotent instance teardown.
- Live-proven durable RuleSet rollback.
- Normal-build approval-bound legacy/manual activation authority (two-person,
  leased, fresh-readiness-at-apply) that rejects product-owned slots.
- PreviewReady-to-approval promotion core: exact inactive publication,
  idempotent durable journal, product-bound approval payload, two-layer journal
  link gate, and no active-pointer mutation during publication or promotion.
- Pure authenticated product application that sequences opaque-session
  authentication, mutation CSRF, fresh Discord authority, atomic server-owned
  session snapshots, promotion, approval, Apply, and exact status projection.
- Hardened product HTTP transport contract with exact-origin checks, secure
  cookie and CSRF boundaries, strict payload parsing, resource limits, stable
  response validation, bounded OAuth-start admission, non-enumerating
  installation authority, and no raw authority-bearing fields.
- Runnable loopback product-control composition with process-only configuration,
  environment or macOS Keychain secret references, thirteen distinct bounded
  database pools, aggregate capability readiness, bind-time deep revalidation,
  explicit database credential provenance, atomic single-owner readiness,
  periodic parameter-ACL and role-setting drift detection, bounded HTTP
  admission and drain, and bounded concurrent pool shutdown. Stable process
  failures expose only closed status, role, and readiness-phase codes. A
  staging-only PostgreSQL manifest reconciles the thirteen exact capability
  roles and fails closed on unexpected privileges or topology.
- Discord identify-only OAuth exchange and fresh bot-observed guild manager
  evidence with bounded write and read lifetimes.
- PostgreSQL product identity, OAuth flow, opaque session and CSRF storage,
  encrypted authoring generations, current and historical installation
  authority, bounded retention, and replay-evidence persistence.
- PostgreSQL installation-authority projection with exact actor/session/install
  binding, database-time lifecycle revalidation, exact current-head selection,
  bounded read-only snapshotting, non-enumerating inactive states, corruption
  detection, and redacted records and failures. Its direct read now runs only
  through a versioned fixed-search-path security-definer function owned by the
  common relation owner. The adapter-exposed readiness contract requires that
  owner to be non-login and verifies the exact
  signature, result shape, owner, ACL, role attributes, schema/database
  capabilities, a direct login session, absence of role memberships and table
  or column privileges, and a real empty-scope execution probe. Migration tests
  also prove hostile default function grants are removed. An isolated non-owner
  role test proves the function-backed
  product path succeeds while direct table reads and writes, schema and
  temporary-table creation, unrelated privileged functions, PUBLIC, and an
  ungranted role fail closed.
- PostgreSQL session-only authentication, mutation authentication, and bounded
  session touch through three versioned fixed-search-path security-definer
  functions owned by the common identity-relation owner. Raw CSRF and OAuth
  verifier digests are never projected to the caller; mutation proof uses a
  constant-time comparison against a SHA-256 tag bound to the session digest.
  The adapter readiness contract verifies exact signatures and result shapes,
  function metadata, one non-login owner, ACLs, direct login and database/schema
  capability, no role memberships, no direct table or column privilege, no RLS,
  and data-independent read and rolled-back touch probes. An isolated non-owner
  role test proves valid authentication and compare-and-set touch while denying
  direct identity DML, schema and temporary-table creation, unrelated function
  execution, ungranted roles, PUBLIC, grant option, hostile default grants,
  function-mode and search-path drift, RLS drift, and owner or caller privilege
  drift.
- PostgreSQL product-decision reads through one seven-input, 49-column,
  fixed-search-path security-definer function. The adapter uses a bounded
  read-committed, read-only transaction, accepts exactly one row, maps every
  identity or target miss to `NotFound`, and preserves Rust classification of
  inactive state and persisted corruption. Reader readiness requires exactly
  the topology and decision-read functions, one restricted common owner, no
  direct table or column access across the 11 data relations plus topology
  identity relation, a trusted schema, one logical database identity, and a
  data-independent zero-row probe. Restricted-role PostgreSQL tests cover the
  positive preview/status path, every caller input, malformed identities,
  bounded lock waits, denied DML/DDL and unrelated execution, metadata and ACL
  drift, hostile inherited grants, and atomic migration failure.
- Authenticated PostgreSQL promotion execution through eight versioned,
  fixed-search-path functions and one dedicated non-owner credential. Prepare,
  inactive publication, approval-environment resolution, activation linking,
  final receipt/audit evidence, replay, and exact legacy repair are bound to the
  current product session and fresh guild authority. The adapter readiness
  contract covers 18 permanent relations, 20 required triggers, 18 trigger
  helpers, the exact constraint and concurrency-arbiter index set, a full
  retained-key scan, and rollback-only denial probes. Restricted-role tests run
  a normal high-level promotion and exact replay plus a progressed legacy repair
  while direct relation and internal-helper access remain denied.
- Payload-bound product approval and serializable atomic Apply with one
  pointer/decision/deployment/receipt/audit commit, exact idempotent replay,
  durable drift supersession, and redacted indeterminate-commit handling. The
  approval executor is function-scoped behind a separate credential with exact
  topology, approval, keyring-coverage, trigger, relation, rollback-probe, and
  cross-pool readiness checks. Apply is independently function-scoped behind a
  seven-function credential covering topology, lock, bounded target-artifact
  projection, finalization, Apply-only keyring coverage, runtime-drain begin,
  and runtime-drain consumption. Its readiness gate verifies 25 direct and
  transitive relations, the exact internal helper and 24-trigger graph, trusted
  schema and role topology, exact executable and argument manifests,
  rollback-only lock, artifact, finalizer, drain-begin, and drain-consumption
  probes, and full reader/approval/Apply three-role composition. A restricted
  direct-login PostgreSQL test completes
  a real Apply and exact replay while direct relation access, DML, DDL,
  temporary objects, unrelated capabilities, PUBLIC grants, metadata drift,
  RLS drift, trigger drift, and incomplete key coverage fail closed.
- Fenced PostgreSQL runtime convergence with exact desired-target digests,
  attestation, serving lease and heartbeat evidence, stale-Live recovery, and
  product status that never equates an Applied pointer with Live.
- Current-versus-historical authority separation: policy-only rotation preserves
  runtime eligibility, while lifecycle, target, binding revision, fingerprint,
  or binding-map mismatch fails closed.
- Exclusive product RuleSet-slot ownership with shared advisory locking,
  commit-time exact-deployment lineage, terminal product activation evidence,
  and Rust plus database rejection of legacy or generic bypass paths.
- Pure conversational Intent IR and deterministic Recipe Compiler for the first
  private-study-room recipe.
- Intent protocol V4 semantic identity: ordered human request evidence; closed
  route semantics and evidence-bound adjudication; distinct compiler-input and
  semantic-intent identities; domain-separated compiled-plan and
  candidate-RuleSet identities; Draft, stage, and complete-transcript bindings;
  and fail-closed V3 or pre-release-V4 snapshot handling.
- Bounded Luna-medium Intent frontiers with strict `codex_chatgpt`,
  `gpt-5.6-luna`, `medium`, ChatGPT-auth, and declared 16,384-token evaluation
  policy pinning: default paths use one call, while an explicit private-room
  detail path uses exactly two calls. Human-grounded request mode, safety,
  locale, room-close authorization, runtime, mandatory-control, capability, and
  detail semantics; dynamic facet
  routing; an isolated flat detail wire; slot-specific exact current-turn
  literal grounding; fail-closed parsing; binding-aware atomic candidates;
  exact recipe simulation; durable SQLite generation CAS; and resumable pending
  decisions remain harness-owned.
- Modal input contracts and server-side normalization before interaction
  effects.
- An evaluation framework with local-unsigned source/binary identity checks,
  strict model and context policy, per-attempt serving metrics, V4 identity-class
  assertions, and clean-source acceptance rules. Historical live V3 samples
  include a 10/10 one-repetition baseline, 12/12 contrast results across three
  repetitions per case, 3/3 full custom-detail results, one copy-only pass, and
  a 14/14 clean-source post-refactor regression over the default and contrast
  cases. They remain historical evidence only. The initial V4 Luna cutover
  canaries passed 4/4 from a clean source. A durable 27-phase orchestrator now
  pins the exact 26-case, 232-sample, 272-turn, 298-call acceptance schedule,
  retry-free gates, worker/source/tooling boundaries, request-counter isolation,
  and atomic evidence. After targeted failure-cluster hardening, the final
  clean-source cohort passed 232/232 rows, all acceptance checks, the exact 298
  model/tool calls, request-counter isolation, retry-free execution, and every
  route/adjudication identity class. The earlier failed cohorts remain immutable
  diagnostic history and are not pooled into the passing denominator. Deferred
  recovery binds the initial failure document and digest in the atomic state
  journal and resumes idempotently across a partial final-artifact write.
- CI guarding the cross-crate safety invariants.

## What Is Not Yet Built

- A production user-facing authoring API or UI. The current harness is a CLI and
  evaluation checkpoint.
- A production release certificate for `tools/starring-api`. The binary and its
  thirteen-role aggregate readiness exist, but real credential provisioning,
  migration/capability evidence, non-interactive Keychain reboot proof,
  staging OAuth, disposable-guild authorization, backup/restore, and failure
  drills remain operator gates before public ingress.
- Least-privilege PostgreSQL deployment roles, restrictive default privileges,
  row policies, and whole-process capability probes. Installation-authority and
  authentication read/touch slices and all three product-decision slices now
  have isolated non-owner, direct-DML denial tests and executable readiness
  probes. The API composition invokes their complete aggregate readiness;
  the checked-in reconciliation manifest is limited to a disposable staging
  cluster. A separately reviewed production credential bootstrap covering the
  remaining PostgreSQL catalog ACL surfaces, an automated isolated-cluster
  manifest gate, and a restored-environment whole-process drill are still
  release blockers.
- A production `tools/starring-runtime` worker that performs the actual Discord
  drain, hydration, panel reconciliation, gateway start, attestation, and
  heartbeat loop. The durable state machine is implemented, but no production
  process currently advances Requested deployments to Live.
- A trusted server-side writer that accepts only harness-validated
  `PreviewReadyArtifactV1` output and advances encrypted PostgreSQL authoring
  generations. Existing product control can start from a pre-seeded durable
  generation but is not yet connected to the Luna harness output.
- An administrative / management API.
- Broader multi-process lease/ownership beyond the single per-request lease.
- A periodic teardown-retry worker (teardown resumes on boot, not on a schedule).
- Compact `RouteId` / custom-id compression (deferred until length is a real
  constraint).
- Any non-Discord adapter.
- Any product recipe beyond `starring.private_study_room@1`.
- Actual typed-planner handoff, structured brainstorming state, recipe editing
  and recompilation, typed multi-turn preference accumulation, and bounded
  Intent transcript compression.
- Whole-action-plan deterministic preflight before the first Discord side
  effect.
- Provisioning-state persistence, compensation, reconciliation, and replay
  idempotency for partial external failures.

## Known Limitations

- The product-control HTTP process is runnable only as a staging control plane.
  It authenticates and authorizes the implemented product operations, but it
  cannot make Requested deployments Live without the separate runtime worker,
  and it cannot accept new Luna designs without a trusted server-side writer.
  Public ingress remains a release decision gated by the operational runbook.
- `interaction-smoke` is non-production manual tooling. It is unavailable
  without its compile feature, requires `STARRING_ALLOW_INTERACTION_SMOKE=1`,
  is marked non-publishable, and accepts only ASCII alphanumeric/underscore
  database names with the `starring_` prefix and an underscore-delimited `test`
  segment. Those gates do not authenticate Discord credentials: never provide
  production bot or guild credentials, and drain every old smoke process before
  a cutover. Production build and deployment manifests must exclude the binary
  and both smoke features entirely.
- The HTTP `/v1/me` database mismatch is resolved: `current_principal` uses the
  session-only projection, while `verify_csrf` uses the distinct mutation
  projection. The production facade and thirteen-role startup composition now
  exercise this boundary, while the production runtime process remains absent.
- Apply can durably reach `RuntimePending`, and tests can drive exact simulated
  attestation to Live. Commercial operation still requires the separate runtime
  worker and real Discord lifecycle integration.
- The `automation-panel-installation-postgres` ignored tests share a guild
  constant and must run serially (`--test-threads=1`); CI does this. A cleaner
  per-test isolation is deferred.
- `last_apply_error` keeps only the latest attempt (no history table). Legacy and
  manual `observed_active` remains informational; product-authoring requests bind
  an exact expected baseline and fail as `Superseded` when it drifts.
- Discord and DB are not jointly atomic; the guarantees are "no durable
  incomplete state" and idempotent convergence, not distributed transactions.
- Layer 1's live end-to-end maturity is not certified here.
- The first recipe checkpoint does not certify commercial readiness. Custom
  copy, naming, and controls now have a passing repeated V4 acceptance matrix,
  but concurrent load, throughput, soak recovery, high availability, a
  production API and identity boundary, live Discord execution, and external
  Discord failure recovery remain uncertified.
- V4 structurally removes the V3 identity defect in which three full-detail
  repetitions produced one RuleSet and one compiled-plan identity but two
  input-intent and semantic-intent hash variants. The deterministic identity
  matrix is implemented, and the final 232-sample Luna V4 cohort established
  repeat stability and its bounded single-worker latency distribution. It does
  not establish concurrent or sustained-load behavior. The V3 rows must not be
  relabeled as V4 evidence.
- The V4 evaluator verifies structural consistency, routing, gates, identity
  classes, exact configured literals, call counts, and latency assertions. Its
  serving check pins the worker-reported provider, model, reasoning effort, and
  authentication mode, not a weights or remote backend-configuration digest.
  The declared 16,384-token context remains an evaluation policy; the worker
  does not attest the remote model's active context window.
- Runtime actions are still prepared and executed one at a time. A later
  deterministic failure can leave an earlier Discord mutation behind.
- V3 Intent snapshots are deliberately incompatible with V4. V6 and V7
  non-Intent snapshots can be promoted at the CLI store edge, but any V6 or V7
  snapshot containing Intent state is rejected. Future protocol, prompt,
  extractor, normalizer, or transcript-projection changes require an explicit
  verified compatibility path or another version rotation.
- V4's stage and transcript digests detect accidental corruption and
  inconsistent local edits, not a malicious writer capable of replacing both
  snapshot bytes and all unkeyed digests. A Keychain-backed authenticated
  envelope remains future work before treating local persistence as
  tamper-authenticated.
- The pure-crate dependency guard scans the direct manifest denylist rather than
  the resolved transitive graph. The current graph is safe, but the invariant
  enforcement should be strengthened before external release.
- The operational authoring model is fixed to `gpt-5.6-luna` at `medium`
  reasoning through the private ChatGPT-auth Codex worker. The retained local
  Gemma artifact is rollback material and cannot be mixed into Luna acceptance
  evidence.
- On this Mac mini, some newly linked Rust host binaries have stalled before
  test process entry while macOS AMFI/trust evaluation runs. The current full
  workspace test, Clippy, and formatting gates completed locally; treat a
  recurrence as a host operational fault and continue requiring independent CI
  before merge.
- The data volume is about 83% used with roughly 36 GiB free. Keep at least
  30 GiB free and recover additional headroom before retaining another large
  build or evaluation cohort; this is an operational capacity floor, not a
  certified production margin.

## Next Phase: Prove Commercial Operation

The direction is no longer an automation-versus-game product fork. The near-term
Harness Track comes first; a separate Stateful Runtime Track follows only after
the authoring and execution boundary is reliable.

The immediate sequence is:

1. Provision and independently verify the thirteen API credentials against a
   restored staging database, then complete the Keychain reboot, OAuth,
   disposable-guild authority, shutdown, backup, and rollback drills in the
   control-plane runbook without enabling customer ingress.
2. Complete least-privilege PostgreSQL owner, API, runtime, and maintenance roles;
   restrictive grants/default privileges; row policies; direct-DML denial; and
   a CI-tested positive/negative capability matrix.
3. Build `tools/starring-runtime` with its separate DB role and bot credential,
   then prove Requested through exact Live and Live-loss recovery against a real
   Discord test guild.
4. Connect a trusted server-side harness writer so only validated and simulated
   Luna output can advance encrypted `PreviewReady` generations.
5. Preserve the passing clean-source cohort and failed diagnostic cohorts as
   separate immutable evidence, then measure queueing, concurrency, saturation,
   soak recovery, and worker high availability before setting a commercial SLO.
6. Add typed multi-turn preference accumulation and the typed-planner handoff
   while preserving the same deterministic candidate gates and no-deploy model
   boundary.
7. Add whole-plan deterministic preflight, compensation, reconciliation, and
   uncertain-external-effect replay before expanding the recipe catalog or
   beginning the separate `StatefulSpec` runtime arc.

The handoff document linked above is authoritative for the detailed ordering,
current evidence, commands, and known maintenance debt.

## Source Documents

- Design specs, plans, and runbooks: `docs/superpowers/{specs,plans,runbooks}/`
  (per-phase rationale; the 16–18f arc and the CI and rollback runbooks).
- Current Intent V4 semantic-identity implementation handoff:
  `docs/superpowers/handoffs/2026-07-15-intent-v4-semantic-identity-handoff.md`.
- Current Luna V4 acceptance and hardening handoff:
  `docs/superpowers/handoffs/2026-07-17-luna-v4-acceptance-hardening-handoff.md`.
- V4 semantic-identity design and deterministic verification contract:
  `docs/superpowers/specs/2026-07-15-intent-v4-semantic-identity-design.md`.
- Historical Gemma V3 Intent/Recipe live checkpoint handoff:
  `docs/superpowers/handoffs/2026-07-15-gemma-intent-cohort-handoff.md`.
- Original Intent IR, Recipe Compiler, persistence, runtime, and server
  checkpoint:
  `docs/superpowers/handoffs/2026-07-14-intent-recipe-checkpoint-handoff.md`.
- Historical, superseded design docs (kept for context, flagged as outdated):
  `docs/discord_ai_control_plane_architecture_oci.md`, `docs/repo-structure.md`.
- `README.md` is the product entry point; this file is the engineering truth
  source.
