# Intent Recipe Session and Serving Design

## Decision

The first recipe-backed authoring path uses one Gemma model call per ordinary user turn and no model calls for RuleSet assembly, validation, simulation, or preview.

```text
user turn
→ one typed route or decision extraction
→ deterministic workspace normalization
→ deterministic recipe compilation
→ atomic candidate execution
→ binding-aware validation
→ exact recipe simulation
→ preview
→ canonical Draft commit
```

The model used by live evaluation and production acceptance is `gemma4:12b-mlx`. Model fallback and mixed-model acceptance reports are forbidden.

## One-call frontier

The OpenAI-compatible Gemma client enables strict `response_format` and promotes JSON text to a tool call only when exactly one tool definition is present. Intent mode therefore exposes one tagged-union tool rather than several competing tools.

The initial frontier is `route_intent_turn`.

```text
private_study_room
typed_planner
capability_gap
reject
discussion
```

The only model-facing harness state is `expected_revision`. The schema never contains the intent schema version, durable workspace, feature identifier, recipe identifier, provenance, raw actions, permission bits, instance manifests, RuleSet JSON, publication, approval, deployment, or activation fields.

An incomplete private-room proposal exposes only `resolve_intent_decision` on the next user turn. The active decision identifier, expected type, options, and workspace remain harness-owned. The model returns the expected revision and one existing channel key.

Malformed arguments, a stale model revision, a wrong tool, multiple calls, plain prose that cannot be promoted, or an LLM transport failure stops the turn after that single model call. Intent mode has no automatic model retry or repair loop.

## Session state machine

```text
Empty
  route private_study_room, complete
    → prepare candidate → commit → PreviewReady
  route private_study_room, missing hub
    → AwaitingDecision
  route fallback
    → Routed

AwaitingDecision
  resolve current channel decision
    → prepare candidate → commit → PreviewReady
  invalid or stale resolution
    → halt without mutation

PreviewReady
  → durable validated preview and recipe ownership receipt
```

The durable intent snapshot records the protocol version, state, normalized partial workspace when present, active deterministic decision, root Draft revision, recipe receipt when ready, and a fingerprint of the exact runtime resource-binding context.

Restore requires the caller to supply the current binding map. The session recomputes the fingerprint and fails closed if any key or resolved Discord identifier differs. A key catalog derived separately from the actual gate bindings is forbidden because the two representations can drift.

The legacy adaptive and typed-plan modes remain available. Intent mode is opt-in and has a distinct fixed system prompt and restore path.

## Cache-friendly prompt construction

The system prompt and selected tool schema form an immutable prefix. Human messages, assistant tool calls, deterministic tool results, and state anchors are appended and never edited in place.

The current state anchor is appended immediately before the model call. Old anchors remain immutable transcript evidence. Context compaction may remove complete old interaction groups only after their semantic result is represented in durable state. It must never orphan an assistant tool call from its result.

The anchor contains only bounded state needed for the current extraction:

- current expected revision;
- stage;
- available channel binding keys;
- one active missing decision when present;
- bounded recent conversational intent.

It excludes the full RuleSet, compiled low-level requirements, validation trace, and simulation trace.

## Atomic candidate boundary

A resolved intent is never applied incrementally to the canonical Draft.

```text
compile
→ verify manifest bindings
→ normalize exact requirements
→ execute against a clone
→ validate with the same bindings
→ simulate every rendered recipe control
→ render strict preview
→ compare-and-swap commit
```

Any parse, resolution, compile, binding, conflict, dispatch, validation, simulation, preview, or stale-root failure leaves the canonical Draft byte-equivalent to its pre-turn value.

Compiled recipe operations are deterministic harness work. They are recorded separately and never counted as model tool calls or typed-planner tool calls.

## Identity and equivalence

`input_intent_hash` remains an audit hash over the exact normalized intent, including revision and value provenance.

`semantic_intent_hash` excludes workspace revision and value provenance while retaining every user-visible semantic, feature identity, pinned recipe version, locale, copy, naming, controls, and external binding key.

A one-shot request may resolve at revision 1 with `ModelExtracted` provenance while the equivalent multi-turn request resolves at revision 2 with `UserConfirmed` provenance. Acceptance requires:

- different exact input hashes;
- identical semantic intent hashes;
- identical compiled plan hashes;
- identical ordered requirements;
- identical final RuleSet values.

## Routed fallbacks

Fallbacks are explicit outcomes, not successful recipe builds.

- `typed_planner` hands a supported custom automation to the existing atomic typed planner;
- `capability_gap` reports required runtime capabilities that do not exist;
- `reject` records a safety-boundary refusal;
- `discussion` continues product brainstorming without Draft mutation.

The public outcome and turn phase use `Routed` rather than overloading `Progressed`. A fallback never claims validation, simulation, preview, or recipe completion.

## Persistence concurrency

SQLite persistence uses a monotonically increasing generation per session.

```text
load → snapshot + generation N
run one turn in memory
save where generation = N
→ success writes generation N+1
→ zero affected rows is a typed conflict
```

Creation succeeds only when the expected generation is zero and the session is absent. Two writers that load the same generation cannot both commit. The CLI reports `Ready` only after the new snapshot is durably persisted. On a persistence conflict it must discard the in-memory outcome, reload, and require a new user turn rather than replaying an LLM result against changed state.

## Observability

Intent mode adds cumulative and per-turn counters for:

- route model calls;
- accepted recipe proposals;
- accepted decision resolutions;
- compile attempts and successes;
- candidate commits and rollbacks;
- root conflicts;
- stale model revisions;
- extraction failures;
- fallback routes by kind;
- deterministic compiled operations.

Every live report records the exact model, context limit, gateway identity, commit, run order, start and end timestamps, turn latency, model calls, model tool calls, deterministic operations, hashes, actual validation and simulation stamps, and postcheck results.

## Commercial runtime hardening

Recipe authoring correctness does not by itself make event execution commercially safe. The current runtime renders templates immediately before each mutating action. A later oversized dynamic panel can therefore fail after a role, channel, overwrites, and member grant already succeeded.

Commercial readiness requires independent feature commits in this order:

1. split runtime execution into pure preparation and effectful execution without behavior change;
2. add bounded modal input contracts and server-side normalization;
3. render and resolve every deterministic action in a preflight stage before the first side effect;
4. make generated channel names instance-scoped so sanitized names cannot collide;
5. compile a new pinned private-room recipe version with exact input and template budgets;
6. add provisioning-state persistence, compensating cleanup, reconciliation, and interaction replay idempotency for failures that occur after external mutations begin.

The first five items prevent deterministic input and rendering failures from creating orphan resources. They do not solve network uncertainty or a Discord failure after a successful earlier mutation. Those require provisioning state and compensation.

## Evaluation

Gemma evaluation includes repeated runs of:

- complete private StudyRoom one-shot;
- the identical design split across a hub clarification;
- Korean and English requests;
- complete requests that must not ask a question;
- missing-hub requests that must ask exactly one deterministic question;
- brainstorming followed by an explicit build turn;
- supported custom flows routed to the typed planner;
- creator-only close and stateful game requests reported as capability gaps;
- disallowed requests rejected without mutation;
- conflicting targets and stale revisions with zero canonical mutation;
- restart while awaiting a decision;
- concurrent persistence writers where exactly one commit succeeds.

Acceptance requires 100% deterministic compiler, validation, simulation, provenance coverage, one-shot and multi-turn structural equivalence, missing-decision behavior, rollback safety, and persistence conflict safety. Known-recipe selection must be at least 9 of 10 runs, complete-request unnecessary questions must be zero, repeated identical error loops must be zero, and the normal known-recipe path must use one model call per user turn.

Latency targets remain end-to-preview P50 below 8 seconds and P95 below 20 seconds, with a 60-second safe halt boundary. Measurements below the required sample count or with mixed models are diagnostic only and cannot support a commercial-ready claim.

## Commit and gate contract

Each responsibility is committed independently:

1. session state and snapshot invariants;
2. one-call orchestration;
3. SQLite generation CAS;
4. CLI mode and durable-save ordering;
5. evaluation schema and assertions;
6. prompt and latency tuning;
7. runtime input and preflight hardening.

Every commit runs formatting, relevant focused tests, workspace tests, Clippy with warnings denied, dependency guards, and JavaScript evaluation checks when applicable. No commit may weaken approval, publication, version pinning, activation, Discord isolation, or event-time LLM boundaries.
