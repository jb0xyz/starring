# Gemma Intent Cohort Handoff

## Document status

This handoff records the current Gemma-only Intent recipe checkpoint on
2026-07-15. It supersedes the Intent-serving and evaluation status in
`2026-07-14-intent-recipe-checkpoint-handoff.md`; the older document remains
useful for the original Intent IR, Recipe Compiler, persistence, runtime, and
home-server background.

The repository-level source of truth remains `CURRENT_STATE.md`. If later work
changes protocol semantics, serving call counts, hash domains, or acceptance
evidence, update both documents.

The active feature branch at this checkpoint is:

```text
feat/gemma-intent-cohort-hardening
```

The V3 detail-serving sequence currently runs through:

```text
6af355d perf(intent): isolate detail extraction context
b3fa202 fix(intent): flatten serving detail patterns
495b6a9 fix(intent): map detail literals explicitly
b486b79 fix(intent): ground recipe detail literals
74732d1 fix(intent): align detail parser with routed schema
f5d0dfe test(intent): prove copy-only detail routing
edad6cb docs(eval): record Gemma Intent cohort evidence
ba64e0a refactor(intent): split recipe detail responsibilities
```

Treat these as orientation points, not as a substitute for checking the branch
HEAD and diff before continuing.

## Executive status

Starring's first known-recipe authoring path now uses Intent protocol V3. Gemma
extracts a compact high-level Core IR. Deterministic Rust code adjudicates
capabilities and safety boundaries, chooses the pinned recipe, normalizes the
semantic input, compiles the RuleSet candidate, validates it, simulates it, and
produces the preview.

The only implemented product recipe remains:

```text
starring.private_study_room@1
```

The default path is the fast path: one model call and one model tool call. A
successful private-study-room request with explicit `copy`, `naming`, or
`controls` customization makes exactly one isolated detail call, for a total of
two model calls and two model tool calls. There is no model repair or retry.

Custom detail extraction is now live-proven for all three facets together and
for a copy-only reduced frontier. The three full-detail repetitions produced an
identical RuleSet and compiled plan. They did not produce one stable semantic or
input hash: those hashes had two variants, most likely because the model-authored
freeform objective is authoritative in the semantic identity even though it
does not change the compiled plan.

This is meaningful harness progress, not commercial certification. The full
ten-repeat-per-case cohort, production API and identity boundary, concurrent
load and queueing measurements, whole-plan side-effect preflight, and external
failure recovery are still incomplete.

## Product and safety direction

The intended product remains a conversational design partner rather than a raw
JSON generator. It must accept both a complete one-shot request and an
incremental conversation. The current checkpoint supports a complete request,
a deterministic hub-channel clarification, and discussion followed by a build
turn. It does not yet accumulate arbitrary recipe preferences as typed durable
state across many turns, nor edit and recompile a preview-ready recipe-owned
region.

The safety contract is unchanged:

```text
AI semantic extraction at design time
→ deterministic capability and boundary adjudication
→ pinned recipe selection
→ deterministic normalization and compile
→ validate
→ simulate
→ preview
→ user approval
→ immutable publication
→ approval-bound activation
→ deterministic event-time runtime
```

Intent tools cannot publish, deploy, activate, call Discord, mutate the
production database, or bypass approval. Event-time LLM calls remain forbidden.
The V3 changes are confined to the design harness and its evaluation edge; they
do not weaken the engine, publication, approval, activation, version-pinning, or
runtime boundaries.

## Implemented V3 request flow

The active known-recipe flow is:

```text
current human turn
→ interpret_intent_core
→ strict compact Core IR
→ deterministic capability and boundary adjudication
   ├─ discussion, rejection, capability gap, or typed-planner classification
   └─ pinned private-study-room recipe
      ├─ no selected detail facet
      │  → deterministic defaults
      └─ selected copy, naming, or controls facet
         → isolated extract_private_study_room_details call
         → routed-schema parse
         → exact current-turn literal grounding
         → deterministic coverage validation
→ typed workspace preparation
→ deterministic Recipe Compiler
→ atomic Draft candidate
→ validate
→ exact recipe simulation
→ preview-ready receipt
```

### Core frontier

The first call exposes only `interpret_intent_core`. Its bounded fields cover
request mode, automation kind, objective, requested outcome, hub binding,
locale, close authorization, runtime requirements, boundary requests,
unclassified requirements, selected detail facets, and a discussion response.

Gemma does not choose a recipe ID, capability status, manifest digest, generated
resource key, permission policy, action order, RuleSet operation, deployment
operation, or safety result. Deterministic code derives the route using this
precedence:

```text
explicit safety boundary → reject
discussion → discussion
unsupported hard requirement → capability gap
supported managed StudyRoom → pinned recipe
other supported static automation → typed planner
```

Capability and boundary adjudication happens before a possible second call. A
creator-only close request, unsupported persistent state or timer, unknown hard
requirement, event-time LLM request, secret disclosure request, live mutation,
or approval bypass cannot spend the detail call or compile a supported subset.

### Model-call contract

- Default StudyRoom, typed-planner classification, discussion, capability gap,
  and rejection use one model call and one model tool call.
- A successful StudyRoom request with at least one explicit custom-detail facet
  uses exactly two model calls and two model tool calls.
- The second call cannot change route, recipe, binding, locale, authorization,
  runtime requirements, safety boundaries, or requested outcome.
- Parallel tool calls are disabled, and Intent serving has no automatic model
  repair or retry.
- A hub-channel decision is a later user turn with its own bounded decision
  frontier; it is not an invisible model retry.

## Active recipe-detail contract

### Dynamic facet routing

The Core call selects zero or more closed detail facets:

```text
copy
naming
controls
```

Only the selected roots are placed in the second tool schema. A copy-only
request exposes only `copy`; it does not expose empty `naming`, `controls`, or
an `unmapped_facets` escape. The router derives the schema deterministically
from the canonical selected facet set.

The same routed schema is reused by the serving parser. The parser requires the
top-level argument keys to equal the selected facet set exactly. Missing,
extra, duplicate, unknown, or empty selected facets fail closed before any
Draft mutation. Tests cover all seven nonempty subsets of the three facets.

The broad public legacy parser retains its optional nested compatibility shape.
That compatibility surface is not the active Gemma wire and must not be used to
weaken the serving schema.

### Flat serving wire

The active detail wire keeps selected `copy`, `naming`, and `controls` objects,
but pattern values are flat sibling fields such as:

```text
channel_name_prefix
channel_name_suffix
member_role_name_prefix
member_role_name_suffix
```

Gemma is not asked to alternate between a scalar and a nested
`{prefix,suffix}` shape. One nonempty affix is sufficient; an omitted partner
becomes an empty string. A present pattern with two empty affixes is rejected.
The typed recipe representation can remain nested behind this serving adapter.

### Isolated second request

The second request contains only:

```text
detail system prompt
current INTENT_HUMAN envelope
current INTENT_DETAIL_STATE anchor
```

It does not receive the first call's Core arguments, Core tool result, unrelated
prior conversation, old detail values, full Draft, compiled plan, or simulation
trace. The durable append-only transcript still records accepted calls and tool
results for audit and restore.

This isolation is a safety and reliability boundary. The harness, not the
model, retains the pinned recipe identity, Core semantic digest, current
revision, and selected facet ticket.

### Literal grounding and coverage

After structural parsing and normalization, every nonempty custom scalar and
every nonempty pattern affix must be an exact, case-sensitive, contiguous
substring of the current raw human turn. CRLF is normalized to LF for this
comparison. Prior turns are not searched.

Grounding runs before the detail digest, evidence creation, recipe
finalization, preparation, or compilation. It proves literal membership, not
semantic attribution when the same literal occurs in more than one legitimate
position.

The harness then stamps revision, Core digest, and covered facets. A selected
facet must contain at least one valid mapped value. Defaults may fill
unrequested fields; they cannot silently fill a requested but missing facet.
Contradictory close controls, unsupported fields, unmapped facets, oversized
values, or values absent from the current turn fail without mutation.

## Deterministic authority and atomicity

For the private-study-room path, deterministic Rust remains authoritative for:

- recipe identity and version;
- capability-manifest and recipe-registry identity;
- stable feature and generated resource keys;
- default copy, naming, and controls;
- permissions, references, action order, and instance manifest;
- normalization and recipe bounds;
- semantic and compiled-plan hashing;
- candidate construction, validation, simulation, and preview;
- Draft revision and SQLite generation compare-and-swap.

The optional detail stage is transient inside one authoring burst. If the detail
transport, parser, grounding, coverage, normalization, compile, validation, or
simulation step fails, the canonical Draft and durable Intent stage remain at
the last stable state. A rejected transcript entry may remain as audit evidence;
it does not create a partially compiled workspace.

Successful recipe evidence binds protocol version, binding fingerprint, Core
digest, adjudication, pinned recipe and registry digest, extraction mode,
selected facets, detail and coverage digests, typed workspace revision, and the
compiler and candidate receipts.

## Evaluation evidence at this checkpoint

All numbers below are local Gemma samples against the fixed
`gemma4:12b-mlx` serving policy. They are different-sized samples and must not
be pooled into one reliability rate.

| Cohort | Sample | Result | Recorded latency | Calls | Meaning |
|---|---:|---:|---:|---:|---|
| Existing ten-case V3 baseline | one repetition per case | 10/10 | not summarized here | one call per ordinary turn | One clean pass over the ten baseline cases, not 10 repetitions per case |
| Four contrast cases | three repetitions per case | 12/12 | not summarized here | 1/1 | Boundary, capability, and contrast routing sample |
| Full copy+naming+controls detail | three repetitions | 3/3 | mean 19,209 ms; P95 19,811 ms | 2/2 | Exact custom literals, 22 operations, current validation and simulation |
| Copy-only reduced frontier | one repetition | 1/1 | 21,610 ms | 2/2 | Only launcher copy changed; naming and control defaults remained; Close stayed absent |
| Post-refactor default+contrast regression | one repetition per case | 14/14 | mean 10,827 ms; P95 17,737 ms | one or two calls per report | Five ready paths preserved 22 operations and current gates; nine routed paths stayed mutation-free |

The corresponding local result files are:

```text
eval/design-harness/results/v3-full10-explicit-gates-1run.json
eval/design-harness/results/v3-contrast4-grounded-3run.json
eval/design-harness/results/v3-custom-details-grounded-3run.json
eval/design-harness/results/v3-copy-only-1run.json
eval/design-harness/results/v3-precustom-final-regression-1run.json
```

`eval/design-harness/results/` is ignored by Git. A fresh clone will not contain
these raw artifacts unless they are transferred separately. Committed
measurements must describe them honestly rather than assuming the JSON files
will be available to another machine.

The full-detail sample compiled all three runs to the same RuleSet and the same
compiled plan hash:

```text
e6cf496594b2b4c4c2c8a7edfd156c431d8684632d8706f0d3135e5b13dd1b5c
```

It produced two input-intent hashes and two semantic-intent hashes. Therefore
the correct statement is:

```text
RuleSet stability observed 3/3
compiled-plan stability observed 3/3
authoritative semantic-identity stability not observed 3/3
```

Do not describe this cohort as semantically hash-stable, deterministic
end-to-end, or commercially accepted.

The design-harness JavaScript suite now contains 44 passing tests. This handoff
does not replace the final branch gate run. Before publishing, rerun the whole
Rust workspace tests, workspace clippy with warnings denied, formatting check,
the 44 JavaScript tests and both Promptfoo configuration validations, then rely
on GitHub Actions for an independent clean-environment result.

The post-refactor report came from clean source and matching build commit
`c1e9a37266df0ae460748c5220402c48be7f5755`. It had no provider error, served
only `gemma4:12b-mlx`, and passed every assertion. This single sample verifies
that the responsibility split preserved the measured default and contrast
paths; it does not change the repeated-acceptance requirement.

## Semantic identity gap

The remaining three-run hash variance is not a RuleSet or compiler-plan
variance. Inspection points to the freeform model-authored `objective` as the
likely source: it participates in the authoritative input and semantic hashes,
while equivalent wording does not change the recipe compiler output.

This should not be patched by silently dropping the field from the existing V3
hash. The next protocol revision should make the distinction explicit:

1. Give a known recipe a harness-owned canonical objective identity, such as a
   closed `objective_id`, for authoritative semantic IR and hashes.
2. Store a model-authored summary as display or conversation annotation only;
   exclude it from route, capability, recipe, compiler, and authoritative hash
   decisions.
3. If users need explicit objective metadata, represent it as a separate typed,
   exact-grounded user field with clear product semantics.
4. Version the semantic digest domain and the protocol/adjudication evidence.
   Do not silently reinterpret V3 snapshots under V4.
5. Add migration or explicit incompatibility behavior, snapshot tests, and
   equivalence tests across default, custom-detail, clarification, and
   discussion-to-build paths.
6. Decide whether extractor, normalizer, compiler, registry, and receipt
   revisions must change from the actual projection changes, and pin them in
   deterministic tests.

The target is not merely three matching hashes. The target is an identity model
where authoritative hashes change exactly when user-visible or safety-relevant
recipe semantics change, and remain stable when only non-authoritative prose
changes.

## Current limitations

- Only one product recipe exists.
- Typed-planner fallback is classified but does not enter an implemented
  typed-planner authoring session.
- Discussion text is retained but is not a typed brainstorming workspace.
- Custom detail extraction reads only the current turn. There is no durable
  typed preference accumulator for details supplied over several turns.
- Preview-ready recipe editing has no `keep`, `set`, and `reset_default` patch
  workflow or atomic owned-region recompile.
- The one baseline pass and small contrast/detail samples do not satisfy the
  ten-repeat-per-case acceptance requirement.
- The semantic/input identity variance above is unresolved.
- The evaluator checks structure, routing, receipts, gates, call counts, exact
  configured literals, validation, simulation, and latency, but model identity
  is still a gateway-reported tag rather than a weights or server-configuration
  digest.
- The harness is a CLI and evaluation edge, not an authenticated production
  authoring API or UI.
- There is no commercial queueing, concurrency, saturation, soak, or recovery
  evidence for the Mac mini deployment.
- Runtime actions still need whole-plan deterministic preflight before the
  first external side effect.
- External Discord failures still need provisioning state, compensation,
  reconciliation, and replay idempotency.
- No Intent path may publish or activate; product integration with the existing
  approval and immutable publication boundary remains future work.

## Ordered continuation

### 1. Correct semantic identity in V4

Write the V4 identity and snapshot design before editing code. Separate
authoritative recipe semantics from display prose, version every affected
digest and receipt domain, preserve fail-closed snapshot behavior, and prove
hash sensitivity and stability deterministically. Then rerun the live custom
detail case at least three times to confirm one semantic, input, plan, and
RuleSet identity under identical user input.

### 2. Add typed multi-turn preferences and edit/recompile

Introduce a harness-owned typed preference workspace that can accumulate
explicit facts across turns without asking Gemma to remember the Draft. Define
provenance, conflict behavior, `keep`, `set`, and `reset_default`, stale
revision handling, and an atomic recompile of the recipe-owned region while
preserving stable feature and generated keys. A full one-shot request and the
equivalent multi-turn conversation should converge on the same authoritative
semantic and compiled-plan identities.

### 3. Complete the typed-planner handoff

The deterministic router already identifies supported custom static automation.
Implement the actual typed-planner session boundary rather than returning only
a classification. Keep recipe and planner ownership distinct, require typed
packets and deterministic review, and preserve the same candidate validation,
simulation, preview, and no-deploy boundary.

### 4. Build the production authoring edge

Add an authenticated API and user-facing conversation surface, authoritative
guild binding, session ownership and concurrency semantics, bounded context and
resume behavior, and integration with immutable publication and the existing
approval-bound activation flow. Do not use the CLI's manual actor value as a
production identity mechanism.

### 5. Prove commercial operation and runtime preflight

Measure queueing, parallel request behavior, saturation, tail latency, and soak
recovery on the Gemma-only Mac mini. Separately add whole-action-plan
deterministic preflight before the first Discord side effect, then provisioning
state and recovery for uncertain external effects. Set an SLO only after these
measurements exist.

### 6. Expand recipes before the stateful-runtime track

Add a second recipe to prove that registry, extractor, normalizer, compiler,
simulator, and ownership boundaries are genuinely reusable. Persistent state,
timers, sessions, economies, and games belong in a separately designed
`StatefulSpec` and runtime arc; they must not be smuggled into the current
RuleSet recipe as ad-hoc fields.

## Continuation rules

- Keep `gemma4:12b-mlx` as the only model in this acceptance line. A different
  model requires separate evidence and must not be pooled with Gemma results.
- Keep API keys and gateway credentials in environment or Keychain only. Never
  place them in source, fixtures, logs, result summaries, or commits.
- Keep `design-harness` pure. HTTP and SQLite remain edge responsibilities.
- Do not change engine, publication, approval, activation, Discord, or database
  safety boundaries as part of Intent quality tuning.
- Preserve feature-sized commits and run relevant focused gates before each
  commit, then the full branch gates before publishing.
- Record failed experiments and regressions. Do not promote a diagnostic sample
  into a reliability or commercial-readiness claim.
- Keep model-authored prose non-authoritative for capability, safety, recipe,
  deployment, and runtime decisions.

## Primary implementation map

The high-churn detail implementation was split without changing behavior at
`ba64e0a`. The parent `intent_recipe_details.rs` is now a 132-line facade and
wire/data declaration module. Responsibility-specific code lives in
`intent_recipe_details/schema.rs`, `parse.rs`, and `validation.rs`. Public and
crate-visible APIs, serialization, schema field order, tool names, structured
errors, and tests remain unchanged. Continue this responsibility-based layout
instead of rebuilding the former 611-line mixed module.

- V3 Core wire and parser:
  `crates/design-harness/src/turn/intent_core.rs`
- Detail wire, schema, parser, and grounding:
  `crates/design-harness/src/turn/intent_recipe_details*`
- Capability adjudication and digests:
  `crates/design-harness/src/session/intent_routing/adjudicate*`
- Session serving, isolated detail request, evidence, and execution:
  `crates/design-harness/src/session/intent_routing/`
- Recipe registry and revisions:
  `crates/design-harness/src/intent/catalog.rs`
- Semantic and compiled-plan hashes:
  `crates/design-harness/src/intent/semantic.rs` and
  `crates/design-harness/src/intent/compile.rs`
- CLI and SQLite edge:
  `tools/design-harness/`
- Intent cases and assertions:
  `eval/design-harness/intent-cases.yaml` and
  `eval/design-harness/intent-assertions.js`
- Measurement history:
  `eval/design-harness/measurements.md`
- V3 protocol rationale:
  `docs/superpowers/specs/2026-07-14-intent-core-recipe-detail-protocol-design.md`

## Final checkpoint interpretation

The high-level Intent IR plus deterministic Recipe Compiler direction is
working for the first known recipe. Harness engineering has reduced the model's
responsibility enough that default routing, contrastive safety behavior, and a
bounded custom-detail path all have successful live Gemma samples. The next
quality ceiling is no longer low-level RuleSet generation. It is the precision
of authoritative semantic identity and durable multi-turn preference handling.

The commercially responsible next move is therefore V4 identity separation,
then typed multi-turn edit/recompile, then a real planner handoff and production
edge. More prompt tuning alone will not close those structural gaps.
