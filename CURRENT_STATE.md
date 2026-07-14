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
Discord with a natural-language frontend." A Rust workspace of **30 crates and 5
tools**, PostgreSQL-backed at its durable runtime boundaries, organized into
three layers. The defining safety principle is constant across all layers:

> AI designs at install/authoring time. The runtime executes deterministically.
> Event-time LLM calls are forbidden (enforced per-crate by `no_ai_gateway`
> tests).

Layer 1 and Layer 2 production runtime boundaries are PostgreSQL-or-die and
fail-closed: they do not start, and do not mutate Discord, on an unsafe or
unverifiable state. Layer 3 keeps local authoring state in SQLite and cannot
touch Discord or the production database.

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
adds Layer 3 above it without changing publication, approval, activation, or
event-time execution. All layers share the Discord domain model and the
resource-binding layer.

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
a discussion followed by an explicit build turn. Intent protocol V3 uses one
bounded Gemma Core extraction for default, discussion, fallback, gap, and
rejection paths. A successful private-study-room request with explicit copy,
naming, or controls customization uses exactly one additional isolated detail
extraction, for two model calls and two model tool calls total. For the
`starring.private_study_room@1` capability, deterministic Rust code owns
capability and safety adjudication, recipe selection, normalization, defaults,
recipe versioning, generated keys, permission policy, references, action order,
instance manifests, validation, simulation, and preview. The model never
assembles the low-level RuleSet for this path.

The active detail router exposes only the selected `copy`, `naming`, and
`controls` roots. Its flat serving wire and parser share the same dynamically
derived schema. The isolated second request sees only the current human turn
and harness-owned detail state, and every accepted custom scalar or pattern
affix must occur as an exact case-sensitive literal in that current turn before
compilation.

The layer currently exists as the pure `design-harness` crate and the
SQLite/HTTP `design-harness-cli` edge. It is a CLI and evaluation checkpoint,
not a production API or UI. The only implemented product recipe is the private
study room recipe. Typed-planner fallback is classified but not yet handed off
to an actual typed-planner session. No Layer 3 authoring path publishes or
activates a design.

The current V3 implementation, evidence, limitations, and ordered continuation
plan are recorded in
`docs/superpowers/handoffs/2026-07-15-gemma-intent-cohort-handoff.md`. The
2026-07-14 handoff remains background for the original Intent IR, Recipe
Compiler, persistence, runtime, and server checkpoint.

## Workspace Topology

30 crates, 5 tools. The recurring pattern is **pure core + edge adapter**: pure
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
- **Layer 2 durable ruleset**: `automation-ruleset` (registry core),
  `automation-ruleset-postgres`, `automation-ruleset-readiness` (hydration +
  activation gate), `automation-ruleset-dispatch` (pinned dispatch),
  `automation-ruleset-activation` + `-postgres` (approval-bound activation
  authority).
- **Layer 2 durable instances**: `automation-instance` + `-postgres`,
  `automation-instance-teardown`, `automation-panel-installation` + `-postgres`.
- **Tools**: `interaction-smoke` (Layer 2 live runner + activation CLI),
  `executor-smoke`, `starring-demo`, `ai-eval`, `design-harness`
  (Gemma/SQLite CLI and evaluation edge).

Persistence is six migrations under `/migrations` (instances, rulesets,
instance-version pin, panel installations, deleting-status, activation-requests).

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

The active RuleSet pointer changes only through the activation authority
(`automation-ruleset-activation`). No CLI, API, or runtime path calls the
low-level activation (`activate_if_ready` / `RuleSetStore::activate`) directly —
a guard test enforces the allowlist. An activation is a durable request bound to
an immutable target (guild, key, version, content hash); it requires a quorum of
**distinct** authenticated approvers (the requester cannot self-approve); the
leased apply service re-verifies the target and runs a **fresh** readiness check
at execution time before mutating the pointer; crashes and concurrent execution
converge through per-request and per-`(guild, key)` CAS. A development-only
`unsafe-dev-activate` escape (compile-feature-gated, absent from normal builds)
skips approval but keeps every technical safeguard.

## Core Safety Invariants

- Layer 1 and Layer 2 production runtime boundaries are PostgreSQL-or-die and
  fail-closed: no start and no Discord mutation on an unsafe or unverifiable
  state. Layer 3 authoring persistence is local SQLite and has no Discord or
  production-database mutation authority.
- AI at design/authoring time only; runtime is deterministic; event-time LLM is
  forbidden (`no_ai_gateway` per crate).
- Pure crates carry no `sqlx`/`twilight` regular dependency (`dependency_guard`).
- One readiness gate shared by boot hydration and activation, so the read side
  and write side cannot diverge.
- Interactions dispatch against the instance's pinned version, re-checking
  readiness against a fresh snapshot per click.
- The active pointer mutates only through the approval-bound activation authority.

## Verification and CI

- **CI** (`.github/workflows/ci.yml`, GitHub Actions, push + PR): a DB-less job
  (fmt, build, `cargo test --workspace`, clippy `-D warnings`, unsafe-dev feature
  build, and design-harness JavaScript/Promptfoo static checks) and a PostgreSQL
  job (six adapter packages' ignored integration tests, serial). No live Discord
  or LLM in CI.
- **Test volume**: the complete Rust workspace suite, 44 design-harness
  JavaScript tests, two Promptfoo configuration validations, and the ignored
  PostgreSQL integration suites for contention, CAS, lease, partial-unique, and
  reconnect behavior.
- **Live certification**: manual runbooks (real bot, guild, PostgreSQL) prove the
  end-to-end lifecycle; they are never wired into CI.

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
- Approval-bound, un-bypassable activation authority (two-person, leased,
  fresh-readiness-at-apply).
- Pure conversational Intent IR and deterministic Recipe Compiler for the first
  private-study-room recipe.
- Bounded Gemma Intent frontiers with strict model pinning: default paths use
  one call, while an explicit private-room detail path uses exactly two calls.
  Dynamic facet routing, an isolated flat detail wire, exact current-turn
  literal grounding, fail-closed parsing, binding-aware atomic candidates,
  exact recipe simulation, durable SQLite generation CAS, and resumable pending
  decisions remain harness-owned.
- Modal input contracts and server-side normalization before interaction
  effects.
- An evaluation framework with local-unsigned source/binary identity checks and
  clean-source acceptance for the fixed Gemma checkpoint. Current live samples
  include a 10/10 one-repetition baseline, 12/12 contrast results across three
  repetitions per case, 3/3 full custom-detail results, one copy-only pass, and
  a 14/14 clean-source post-refactor regression over the default and contrast
  cases. The required ten-repeat-per-case live cohort is not yet complete.
- CI guarding the cross-crate safety invariants.

## What Is Not Yet Built

- A production user-facing authoring API or UI. The current harness is a CLI and
  evaluation checkpoint.
- An authenticated production approval surface (the CLI's manual actor input is
  workflow validation only, not identity assurance).
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
- A versioned separation between authoritative semantic identity and
  non-authoritative model-authored objective or display prose.
- Whole-action-plan deterministic preflight before the first Discord side
  effect.
- Provisioning-state persistence, compensation, reconciliation, and replay
  idempotency for partial external failures.
- A clean-source ten-repeat-per-case Gemma Intent acceptance result.

## Known Limitations

- Manual CLI `--actor` input provides no production identity assurance; the real
  approval boundary completes when an authenticated surface attaches.
- The `automation-panel-installation-postgres` ignored tests share a guild
  constant and must run serially (`--test-threads=1`); CI does this. A cleaner
  per-test isolation is deferred.
- `last_apply_error` keeps only the latest attempt (no history table);
  `observed_active` is recorded but does not gate apply.
- Discord and DB are not jointly atomic; the guarantees are "no durable
  incomplete state" and idempotent convergence, not distributed transactions.
- Layer 1's live end-to-end maturity is not certified here.
- The first recipe checkpoint does not certify commercial readiness. Custom
  copy, naming, and controls now have bounded live samples, but the repeated
  acceptance matrix, separate-process restart, concurrent load, throughput,
  soak recovery, production API and identity boundary, and external Discord
  failure recovery remain incomplete.
- The three full-detail repetitions produced one RuleSet and one compiled-plan
  identity, but two input-intent and semantic-intent hash variants. The likely
  source is equivalent freeform model-authored objectives. Protocol V4 must
  separate authoritative recipe identity from non-authoritative display prose
  instead of silently weakening the V3 hash.
- The checkpoint verifies structural consistency, routing, gates, equivalence,
  call counts, and latency assertions, but has no independent oracle for the
  requested hub, locale, objective, or copy. Its model check pins the exact
  gateway-reported tag, not a weights or server-configuration digest.
- Runtime actions are still prepared and executed one at a time. A later
  deterministic failure can leave an earlier Discord mutation behind.
- Snapshot V5-to-V6 migration is implemented at the CLI store edge, not in the
  public `DesignSession::restore` API. New modal contract fields can also be
  rejected by an older strict V1 reader. Both are rollout compatibility risks.
- The pure-crate dependency guard scans the direct manifest denylist rather than
  the resolved transitive graph. The current graph is safe, but the invariant
  enforcement should be strengthened before external release.
- The operational model is fixed to `gemma4:12b-mlx`; other local model
  artifacts may remain on disk but cannot be mixed into acceptance evidence.

## Next Phase: Measure and Harden the Harness

The direction is no longer an automation-versus-game product fork. The near-term
Harness Track comes first; a separate Stateful Runtime Track follows only after
the authoring and execution boundary is reliable.

The immediate sequence is:

1. Design and implement a versioned Intent protocol V4 that gives known recipes
   a harness-owned canonical semantic identity and keeps model-authored display
   prose out of authoritative hashes. Define snapshot compatibility and digest
   revisions before changing code.
2. Add typed multi-turn preference accumulation plus atomic recipe-owned-region
   edit and recompile with explicit `keep`, `set`, and `reset_default`
   semantics.
3. Complete the actual typed-planner handoff for supported custom static
   automation while preserving the same candidate gates and no-deploy boundary.
4. Run the exact clean-source Gemma checkpoint with at least ten samples for
   every acceptance case, without weakening assertions or mixing models.
5. Add an authenticated authoring API/UI, guild-binding authority, session
   ownership, and the existing approval/publication/activation integration.
6. Measure queueing, concurrency, saturation, and recovery on the Gemma-only
   Mac mini before setting a commercial SLO.
7. Add whole-plan deterministic preflight before the first side effect, then
   provisioning state, compensation, reconciliation, and replay idempotency for
   uncertain external effects.
8. Release a bounded next private-room recipe, expand the recipe catalog, then
   begin the separate `StatefulSpec` runtime arc for state, conditions, timers,
   sessions, and games.

The handoff document linked above is authoritative for the detailed ordering,
current evidence, commands, and known maintenance debt.

## Source Documents

- Design specs, plans, and runbooks: `docs/superpowers/{specs,plans,runbooks}/`
  (per-phase rationale; the 16–18f arc and the CI and rollback runbooks).
- Current Gemma Intent/Recipe checkpoint handoff:
  `docs/superpowers/handoffs/2026-07-15-gemma-intent-cohort-handoff.md`.
- Original Intent IR, Recipe Compiler, persistence, runtime, and server
  checkpoint:
  `docs/superpowers/handoffs/2026-07-14-intent-recipe-checkpoint-handoff.md`.
- Historical, superseded design docs (kept for context, flagged as outdated):
  `docs/discord_ai_control_plane_architecture_oci.md`, `docs/repo-structure.md`.
- `README.md` is the product entry point; this file is the engineering truth
  source.
