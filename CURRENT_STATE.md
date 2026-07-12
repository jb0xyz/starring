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
Discord with a natural-language frontend." A Rust workspace of **29 crates and 4
tools**, PostgreSQL-backed, organized into two planes. The defining safety
principle is constant across both planes:

> AI designs at install/authoring time. The runtime executes deterministically.
> Event-time LLM calls are forbidden (enforced per-crate by `no_ai_gateway`
> tests). The system is PostgreSQL-or-die and fail-closed: it does not start, and
> does not mutate Discord, on an unsafe or unverifiable state.

## Architecture at a Glance

```
Layer 1 — DesiredState Control Plane (one-shot guild configuration)
  natural language ─▶ DesiredState ─▶ diff ─▶ policy ─▶ preview ─▶ approval ─▶ execute

Layer 2 — Durable Interaction Rule Plane (dynamic, per-interaction automation)
  RuleSet authoring ─▶ publish (immutable version) ─▶ approval-bound activation
    ─▶ readiness-gated hydration ─▶ pinned per-interaction dispatch ─▶ teardown
```

Recent work (the engine arc this document primarily certifies) is Layer 2. Both
planes share the Discord domain model and the resource-binding layer.

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

The dynamic plane, and the focus of the 16–18f arc. Server operators (today via
authored fixtures; tomorrow via the harness) define an `InteractionRuleSet` —
panels, buttons, modals, and rules that react to interactions (a "Create study
room" button, a "join" button, a "close" button). A ruleset is published as an
immutable, content-addressed, versioned artifact, activated behind an approval
boundary, hydrated at boot behind a readiness gate, and thereafter each
interaction is dispatched deterministically against the exact ruleset version its
instance was born with. Rooms (instances) own their complete Discord footprint
and can be torn down durably. This plane has been proven live end-to-end,
including RuleSet rollback and approval-bound activation, and is guarded by CI.

## Workspace Topology

29 crates, 4 tools. The recurring pattern is **pure core + edge adapter**: pure
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
- **Layer 2 durable ruleset**: `automation-ruleset` (registry core),
  `automation-ruleset-postgres`, `automation-ruleset-readiness` (hydration +
  activation gate), `automation-ruleset-dispatch` (pinned dispatch),
  `automation-ruleset-activation` + `-postgres` (approval-bound activation
  authority).
- **Layer 2 durable instances**: `automation-instance` + `-postgres`,
  `automation-instance-teardown`, `automation-panel-installation` + `-postgres`.
- **Tools**: `interaction-smoke` (Layer 2 live runner + activation CLI),
  `executor-smoke`, `starring-demo`, `ai-eval`.

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

- PostgreSQL-or-die, fail-closed: no start and no Discord mutation on an unsafe
  or unverifiable state.
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
  build) and a PostgreSQL job (six adapter packages' ignored integration tests,
  serial). No live Discord or LLM in CI.
- **Test volume**: ~535 workspace tests plus ~30 ignored PostgreSQL integration
  tests (contention, CAS, lease, partial-unique, reconnect).
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
- CI guarding the cross-crate safety invariants.

## What Is Not Yet Built

- A user-facing product harness / UI (the next major direction).
- An authenticated production approval surface (the CLI's manual actor input is
  workflow validation only, not identity assurance).
- An administrative / management API.
- Broader multi-process lease/ownership beyond the single per-request lease.
- A periodic teardown-retry worker (teardown resumes on boot, not on a schedule).
- Compact `RouteId` / custom-id compression (deferred until length is a real
  constraint).
- Any non-Discord adapter.
- A natural-language RuleSet authoring product surface (the harness vision).

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

## Next Phase: Product Harness

The engine and its safety boundary are complete and live-proven. The next
direction is the product harness — a conversational designer that builds a draft
ruleset through tool use, never touching live Discord, and lands changes only
through the existing gates (validate → policy → preview → approval-bound
activation). The first decision is a **product fork**: a Discord *automation*
designer (recommended near-term) versus a *game* designer (which would reintroduce
the conditions/state/timer/session runtime Starring deliberately excluded). See
the harness-direction notes for the staged plan.

## Source Documents

- Design specs, plans, and runbooks: `docs/superpowers/{specs,plans,runbooks}/`
  (per-phase rationale; the 16–18f arc and the CI and rollback runbooks).
- Historical, superseded design docs (kept for context, flagged as outdated):
  `docs/discord_ai_control_plane_architecture_oci.md`, `docs/repo-structure.md`.
- `README.md` is the product entry point; this file is the engineering truth
  source.
