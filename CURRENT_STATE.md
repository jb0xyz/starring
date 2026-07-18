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
Discord with a natural-language frontend." A Rust workspace of **33 crates and 5
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

The intended `authoring-application` route loads an owned, revalidated
`PreviewReadyArtifactV1` through trusted session ports and passes it to the pure
`authoring-promotion` workflow. That workflow publishes its exact
`InteractionRuleSet` as an inactive immutable version, creates a
`ProductAuthoring` activation request, and durably binds that request to the
exact promotion journal. The lower-level workflow remains a public core API and
depends on production composition to keep direct transport access unreachable.
The
`authoring-promotion-postgres` adapter persists the monotonic workflow. A
database trigger rejects a product activation link unless the exact
`ActivationPending` promotion row, request, target, requester, policy, payload,
and approval-context identities agree. Approval and apply continue through the
existing activation authority; publication and promotion never change the
active pointer.

The pure `authoring-application` boundary accepts only an idempotency key,
session ID, and expected generation from the client-facing command. It receives
an in-process trusted-principal assertion and delegates owner and generation
checks to owned-session and server-authority ports. The latter resolves guild,
installation, RuleSet key, requester, binding revision, and policy. The boundary
then starts the promotion, attempts advancement, and returns the resulting state,
which may also be terminal expiry. Durable session, authority, HTTP
authentication, approval, rejection, and apply adapters are not yet attached,
so this is an authentication-ready internal contract rather than a production
endpoint.

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

33 crates, 5 Rust tools. The recurring pattern is **pure core + edge adapter**: pure
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
- **Layer 3 to Layer 2 promotion**: `authoring-application` (pure trusted-edge
  application contract), `authoring-promotion` (workflow and product activation
  bridge), and `authoring-promotion-postgres` (durable journal).
- **Layer 2 durable ruleset**: `automation-ruleset` (registry core),
  `automation-ruleset-postgres`, `automation-ruleset-readiness` (hydration +
  activation gate), `automation-ruleset-dispatch` (pinned dispatch),
  `automation-ruleset-activation` + `-postgres` (approval-bound activation
  authority).
- **Layer 2 durable instances**: `automation-instance` + `-postgres`,
  `automation-instance-teardown`, `automation-panel-installation` + `-postgres`.
- **Rust tools**: `interaction-smoke` (Layer 2 live runner + activation CLI),
  `executor-smoke`, `starring-demo`, `ai-eval`, `design-harness`
  (Luna-medium/SQLite CLI and evaluation edge). `codex-worker` is a separate
  private loopback ChatGPT-login Codex service rather than a workspace member.

Persistence is fourteen migrations under `/migrations`, including the original
instance and RuleSet stores, product-bound activation context and terminal
states, the authoring promotion journal, guarded activation, and the exact
promotion-journal link gate.

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

Product-authored requests begin `Unlinked`. Both the pure bridge and a
PostgreSQL trigger require an exact `ActivationPending` promotion journal before
the request becomes approvable. Approval binds the exact promotion payload,
resource projection, policy, and observed active baseline. Apply reloads the
fresh environment and rechecks binding revision, fingerprint, baseline, and
readiness even when the requested target is already active.

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
- The active pointer mutates only through the approval-bound activation authority.

## Verification and CI

- **CI** (`.github/workflows/ci.yml`, GitHub Actions, push + PR): a DB-less job
  (fmt, build, `cargo test --workspace`, clippy `-D warnings`, unsafe-dev feature
  build, and design-harness JavaScript/Promptfoo static checks) and a PostgreSQL
  job (five adapter packages' ignored integration tests, serial). No live Discord
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
- PreviewReady-to-approval promotion core: exact inactive publication,
  idempotent durable journal, product-bound approval payload, two-layer journal
  link gate, and guarded active-pointer mutation through the existing authority.
- Pure authentication-ready authoring application contract that derives
  promotion authority from trusted-edge principal and server-owned session
  ports.
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
- Durable owned-session and guild-installation authority adapters for
  `authoring-application`; its pure ports are implemented but not wired to an
  authenticated HTTP edge.
- An authenticated production approval surface (the CLI's manual actor input is
  workflow validation only, not identity assurance).
- Promotion status convergence after approval, rejection, expiry, apply, and
  runtime hydration. The activation request is authoritative today; promotion
  stages stop at `ActivationPending` or `Expired`.
- Runtime activation acknowledgement and controlled reload/hot-swap. A guarded
  active-pointer change is not yet a `Live` product acknowledgement.
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

- Manual CLI `--actor` input provides no production identity assurance; the real
  approval boundary completes when an authenticated surface attaches.
- `authoring-application` is an internal trust-boundary contract, not an
  authenticator. `VerifiedPrincipalV1::from_trusted_edge` must be called only
  after transport authentication, and production composition must not expose
  `PromotionService::start` directly.
  `OwnedPreviewReadyArtifactV1::from_owned_session` is likewise a public
  in-process trust constructor, not ownership proof by itself. The owned-artifact
  and authority adapters must enforce the same durable generation in one
  transaction or equivalent snapshot before this boundary is
  production-authenticated.
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
- On this Mac mini, some newly linked Rust host binaries can stall before test
  process entry while macOS AMFI/trust evaluation runs. Relevant scoped gates
  are green, but this operational issue prevents claiming that the latest full
  workspace gate completed locally and should be resolved before relying on
  repeated whole-workspace runs.
- The data volume is about 86% used with roughly 31 GiB free. Recover at least
  another 20 GiB before retaining additional large build or evaluation cohorts;
  this is an operational capacity target, not a certified production margin.

## Next Phase: Prove Commercial Operation

The direction is no longer an automation-versus-game product fork. The near-term
Harness Track comes first; a separate Stateful Runtime Track follows only after
the authoring and execution boundary is reliable.

The immediate sequence is:

1. Preserve the passing clean-source cohort and the earlier failed or
   interrupted diagnostic cohorts as immutable, separate evidence. Keep the raw
   worker loopback-only and retain exact provider/model/effort/auth pinning.
2. Measure queueing, concurrency, saturation, long-running soak recovery, and
   worker high availability on the Mac mini before setting a commercial SLO.
3. Run the existing live Discord and PostgreSQL safety scenarios for the
   integrated authoring-to-approval path; the passing authoring matrix itself
   did not exercise Discord.
4. Add typed multi-turn preference accumulation plus atomic recipe-owned-region
   edit and recompile with explicit `keep`, `set`, and `reset_default`
   semantics.
5. Complete the actual typed-planner handoff for supported custom static
   automation while preserving the same candidate gates and no-deploy boundary.
6. Add an authenticated authoring API/UI, guild-binding authority, session
   ownership, and the existing approval/publication/activation integration.
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
