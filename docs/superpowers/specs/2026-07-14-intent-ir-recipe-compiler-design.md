# Intent IR and Recipe Compiler Design

## Decision

Starring will add a high-level authoring layer above the existing typed turn plan.

The model will interpret the conversation, choose a supported feature recipe, and fill bounded semantic slots. A deterministic compiler will expand the resolved intent into exact `ScopeRequirement` values. The existing plan normalization, candidate execution, scope checking, validation, simulation, preview, approval, publication, and activation boundaries remain authoritative.

```text
natural-language conversation
→ IntentWorkspace
→ deterministic resolution
→ ResolvedIntent
→ deterministic Recipe Compiler
→ ordered Vec<ScopeRequirement>
→ existing atomic candidate pipeline
→ validate
→ simulate
→ preview
→ user approval
→ existing publication and activation boundary
```

This is not a second RuleSet language. `InteractionRuleSet` remains the runtime artifact. Intent IR is an authoring artifact and `ScopeRequirement` remains the exact postcondition contract used by the design harness.

## Product objective

The authoring experience must support both forms without separate implementations.

- A complete one-shot request compiles without an unnecessary question.
- A conversational request accumulates confirmed decisions across turns and compiles when complete.
- A brainstorming turn records candidate product intent without mutating the Draft.
- A later edit changes semantic slots and recompiles the recipe-owned region.
- Unsupported capabilities are reported instead of silently omitted.

The known-recipe path must remove low-level RuleSet assembly from the model. The model must never have to spell out permission bits, created references, instance manifests, action order, or raw template expressions for a supported recipe.

## Model policy

Live evaluation for this work uses only `gemma4:12b-mlx` through the existing OpenAI-compatible gateway. No result from another model may be mixed into the acceptance report.

The model remains responsible for:

- conversational interpretation;
- brainstorming and product explanation;
- identifying requested capabilities;
- selecting from harness-provided recipe candidates;
- extracting explicit semantic values;
- naturally presenting a deterministic missing decision;
- summarizing the validated preview.

Deterministic code remains responsible for:

- recipe availability and version resolution;
- required-slot detection;
- safe defaults;
- stable feature identifiers and generated keys;
- template construction;
- action order;
- permission policy;
- created-reference and manifest wiring;
- compilation, conflict detection, and provenance;
- candidate execution, validation, simulation, and rollback.

## Layering

The first implementation is a pure module inside `design-harness` because `ScopeRequirement` currently belongs to that crate and the harness is the only consumer.

```text
crates/design-harness/src/intent/
  mod.rs
  model.rs
  normalize.rs
  keyspace.rs
  catalog.rs
  compile.rs
  provenance.rs

crates/design-harness/src/turn/
  intent_protocol.rs

crates/design-harness/src/session/
  intent_routing.rs
```

The intent and compiler modules must not depend on the LLM client, session transcript, SQLite, network, Discord, publication, or activation. Their public behavior must be deterministic pure functions over typed values and the baseline Draft.

Extraction to `automation-intent` and `automation-recipe` crates is deferred until a second non-harness consumer exists. The internal API must remain shaped so that extraction does not change semantics.

No engine, runtime, persistence, publication, approval, or activation crate changes are part of the first slice.

## Intent workspace

The conversation writes to a partial workspace. Compilation only accepts a resolved value.

```rust
pub struct IntentWorkspaceV1 {
    pub schema_version: u16,
    pub revision: u64,
    pub objective: String,
    pub requested_outcome: IntentRequestedOutcome,
    pub features: Vec<FeatureIntentV1>,
}

pub struct FeatureIntentV1 {
    pub feature_id: FeatureId,
    pub recipe: RecipeRef,
    pub configuration: FeatureConfigurationV1,
}

pub struct RecipeRef {
    pub id: String,
    pub version: u32,
}

pub enum FeatureConfigurationV1 {
    ManagedPrivateRoom(ManagedPrivateRoomDraftV1),
}
```

The serialized envelope has a stable recipe ID and exact version, but it is trusted harness state rather than a model-facing schema. The registry immediately deserializes and validates configuration into a closed Rust type. An unchecked generic JSON payload never reaches the compiler.

Semantic values carry provenance assigned by the harness.

```rust
pub struct IntentValue<T> {
    pub value: T,
    pub source: IntentValueSource,
}

pub enum IntentValueSource {
    ModelExtracted,
    UserExplicit,
    UserConfirmed,
    ContextDerived,
    RecipeDefault,
}
```

The model never submits `schema_version`, `revision`, `feature_id`, recipe identity, or `IntentValueSource`. Narrow recipe-specific input types accept semantic values only. The first input is `PrivateStudyRoomProposalV1`, containing the objective, requested outcome, optional hub and locale, and optional copy, naming, and control overrides. The harness owns revision increments, allocates identifiers, selects the exact registered recipe, and stamps provenance according to whether a value was model-extracted from the user turn, explicitly confirmed by the user, supplied by deterministic context, or materialized from the recipe default. `ModelExtracted` is not treated as user confirmation. Durable workspace JSON is never used directly as a Gemma tool schema.

Only fields with no safe deterministic value remain absent in the partial workspace. Resolution returns either an opaque `ValidatedIntentV1` or a stable ordered list of `MissingDecision` values. The compiler accepts only the opaque value issued by normalization, so callers cannot construct a resolved value and bypass validation. A discussion outcome is never compilable and cannot mutate the Draft.

## Validation and normalization

Intent normalization is fail-closed and deterministic.

- Trim semantic text while preserving intentional internal whitespace.
- Reject empty or oversized objective, labels, content, and resource binding keys.
- Reject unknown intent schema versions, recipe IDs, recipe versions, and fields.
- Reject duplicate feature IDs.
- Reject invalid feature IDs before key derivation.
- Reject unsupported raw templates and control syntax in literal slots.
- Reject raw actions, permissions, manifests, Discord IDs, secrets, scripts, webhooks, expressions, deployment, publication, and activation data.
- Materialize every optional recipe default before compilation.
- Sort missing decisions by feature order and recipe-declared priority.
- Never interpret a missing safety decision as consent.
- Permit exactly one feature in Intent V1 so one compiled transaction remains within the existing 32-requirement atomic-plan bound.
- Validate every external binding against the deterministic channel-binding catalog supplied by the session.
- Preserve the harness-owned revision during pure normalization; revision changes happen only in the later compare-and-swap patch API.

Feature IDs are allocated by the harness and remain stable across copy edits. Generated keys derive from `(feature_id, local_symbol)` and never from presentation text.

## First recipe

The first catalog entry is one composite recipe.

```text
starring.private_study_room@1
```

It is intentionally not exposed as a collection of micro-recipes. Internal helper functions may be decomposed, but the model selects and configures one product-level capability.

The only required external slot is:

- `hub_channel`: existing channel binding key

The locale is an explicit or context-derived `en` or `ko` slot. If neither exists, V1 deterministically uses English and records recipe-default provenance. All default copy for a feature comes from one locale set.

Optional semantic copy and naming slots have deterministic defaults:

- launcher panel content;
- create button label;
- modal title;
- room-name field label;
- channel-name prefix and suffix;
- member-role prefix and suffix;
- welcome content;
- hub announcement content;
- help and join labels;
- optional close label;
- completed, joined, helped, and closed response content;
- close policy: disabled or any member.

The room-name placeholder is semantic. The model supplies prefix and suffix literals while the compiler alone creates `${input.room_name}`.

The recipe fixes these invariants:

- one required short `room_name` modal field;
- `DeferEphemeral` is the first submit action;
- exactly one role and one channel are created;
- everyone is denied `VIEW_CHANNEL` on the created channel;
- the created member role is allowed `VIEW_CHANNEL`;
- the actor receives the created member role;
- the welcome panel is posted to the created channel;
- the discovery panel is posted to the bound hub channel;
- every rendered button has a matching rule;
- help is a static action with an ephemeral response;
- join is an instance action that grants the instance member-role alias;
- close is absent by default;
- close is compiled only for an explicit any-member policy and tears down the event instance;
- the instance manifest contains the created role, channel, welcome message, and hub message exactly once;
- registration follows all owned resource creation;
- `EditResponse` is the final submit action;
- no privileged arbitrary permission slot exists.

The runtime does not currently expose a creator-only authorization predicate for instance actions. The recipe must not label an any-member teardown as owner-only. A creator-only close request is an explicit capability gap until the stateful or authorization runtime adds that predicate.

The existing `studyroom_full` fixture is a creation and registration baseline, not the production acceptance oracle. It renders Help, Join, and Close buttons without matching handler rules. The recipe may reuse its proven creation ordering, but production acceptance requires zero dead buttons and direct simulation of every rendered control.

## Recipe output

Compilation produces an exact ordered plan and a sidecar manifest.

```rust
pub struct CompiledIntent {
    pub requirements: Vec<ScopeRequirement>,
    pub coverage: Vec<IntentCoverage>,
    pub manifest: CompilationManifest,
    pub verification: IntentVerification,
}
```

The requirements are passed through the existing `normalize_turn_plan` before execution. The compiler does not claim that compilation replaces validation.

Coverage maps each high-level clause to the exact generated requirement IDs. For example:

```text
private_membership
→ create_role
→ create_channel
→ deny_everyone_view
→ allow_member_view
→ grant_creator
```

The manifest remains outside `InteractionRuleSet` and contains:

- canonical resolved intent;
- intent schema version;
- exact recipe IDs and versions;
- compiler revision;
- registry digest;
- input intent hash;
- output plan hash;
- generated object path to feature and local-symbol ownership;
- external binding requirements.

The compiler is implemented in two internal milestones on the same opt-in branch. The first proves deterministic expansion of the creation core. The recipe is not considered complete until Help and Join handlers are included, every rendered button has a matching rule, and the existing typed plan path can lower the narrow instance-event role reference required by Join. The optional any-member Close handler is included only when that policy is explicit.

## Narrow instance-event seam

The runtime and scope model already represent an event instance role reference. The model-authored packet path does not currently accept it. The only additional input form permitted for the complete managed-room recipe is equivalent to:

```rust
ReferenceInput::InstanceEvent { alias: String }
```

It lowers only to:

```rust
RoleRef::Instance {
    instance: InstanceRef::Event,
    alias,
}
```

Generic created-instance lookup, arbitrary cross-rule created references, and user-supplied raw `InstanceRef` values remain unavailable.

## Composition

The first slice permits exactly one recipe instance and no cross-recipe wiring. Composition is introduced only after one recipe works end to end and requirement budgeting is defined across transaction boundaries.

Future recipe connections use typed output ports, never generated string keys.

```json
{
  "from": {
    "feature_id": "study_rooms",
    "output": "instance"
  }
}
```

Ports carry type and lifetime scope. A resource created within one rule cannot be referenced by another event. Cross-event access must use a typed instance resource alias. Dependency cycles, type mismatches, missing producers, duplicate producers, and key collisions fail before RuleSet lowering.

Recipes cannot expose a generic `append_actions` extension. A future extension point must be named, typed, ordered, and restricted to an allowlisted action family.

## Ownership and edits

Generated objects are recipe-owned through the sidecar manifest.

- A recipe-owned feature is edited by changing semantic slots and recompiling its owned region.
- A low-level manual edit to a recipe-owned region is rejected.
- A user may explicitly detach a feature into a custom RuleSet region after a warning and preview.
- Detached custom regions no longer receive recipe migrations.
- Existing objects without provenance are custom and must never be guessed into recipe ownership.

The first slice installs a new recipe into an empty or non-conflicting Draft. Atomic replacement of an existing recipe-owned region is a later independent feature with its own typed absence and replacement postconditions.

## Versioning

Four versions remain independent:

- Intent schema version;
- Recipe version;
- Compiler revision;
- RuleSet schema version.

`latest` may be used during recipe search but never appears in canonical Intent. Stored intent pins an exact recipe version. A recipe version is immutable after release and is protected by a golden compiled fingerprint test.

An upgrade is explicit:

```text
pinned v1 intent
→ typed migration to v2
→ compile a new candidate
→ validate and simulate
→ show RuleSet and intent diff
→ user approval
→ publish a new RuleSet version
```

Existing active RuleSets and pinned instances remain unchanged.

## Conversation protocol

Intent mode is opt-in during rollout and has a fixed system prefix distinct from legacy and typed-plan modes.

The model-facing frontiers are deliberately small. None accepts the durable workspace envelope or provenance fields.

```text
no selected recipe
→ choose_supported_intent

selected incomplete private-room recipe
→ configure_private_study_room

deterministic missing decision
→ resolve_intent_decision

resolved intent
→ automatic compile, atomic execute, validate, simulate, and preview

preview ready
→ finish_turn
```

The compiler and gates are not model-selected tools in the recipe path. A complete one-shot request can fill the feature in one model response. A multi-turn request applies patches to the same workspace. Both must normalize to the same resolved intent and plan hashes.

Every patch carries an expected harness-issued revision. A stale revision fails without changing the workspace. The harness increments revision exactly once for an accepted semantic change and persists the workspace, active decision, recipe binding, and compilation ownership atomically.

Clarification is permitted only when a value materially changes access, visibility, lifecycle, deletion, external target selection, or another user-visible semantic and no safe reversible default exists. Cosmetic text uses recipe defaults and remains visible in preview.

The harness asks at most one deterministic missing decision per human turn. It never asks whether to continue, compile, validate, or preview.

## Fallback routing

Routing is explicit and observable.

```text
exact recipe available
→ recipe compiler

typed recipe composition available
→ recipe linker and compiler

no recipe but current primitives can express the request
→ existing typed atomic planner

requested capability needs the future stateful runtime
→ capability gap

request violates the safety boundary
→ reject
```

Raw RuleSet JSON is never accepted as a recipe slot. Unsupported features are not silently reduced. A mixed supported and unsupported request compiles the supported subset only after explicit user agreement.

The existing typed planner remains the safe custom-flow path. It is not removed or weakened.

## Safety invariants

The recipe path must preserve all existing boundaries.

- No Discord, HTTP, database, publication, deployment, or activation access exists in the compiler.
- The compiler only creates a candidate plan.
- The existing atomic candidate root is preserved until all requested gates pass.
- Any parse, resolution, compile, conflict, dispatch, postcondition, validation, simulation, or preview failure leaves the canonical Draft unchanged.
- Existing validator, readiness, content hash, approval, version pinning, and activation checks remain mandatory.
- Event-time LLM remains forbidden.
- The recipe registry is a statically linked first-party allowlist in the first slice.
- Recipe expansion has a declared maximum requirement count checked before execution.
- Generated diagnostics are stable and sorted.

## Performance targets

The known-recipe path targets:

- one model call for a complete one-shot request, with two as the hard normal maximum;
- one model call per ordinary conversational user turn;
- zero model calls for RuleSet expansion;
- deterministic resolve, compile, conflict checking, and plan generation below 50 ms locally;
- deterministic compile through validate and preview below 500 ms locally;
- end-to-preview P50 below 8 seconds and P95 below 20 seconds with the local Gemma gateway;
- an interactive hard limit of 60 seconds before safe halt or asynchronous handoff;
- no unbounded retry or same-error loop.

Prompt and tool schemas must remain cache-friendly. Recipe selection exposes only recipe summaries relevant to the current capability request. Recipe configuration exposes only the selected recipe schema.

## Evaluation contract

All live samples use `gemma4:12b-mlx` and record the exact model, context, gateway, commit, run order, and timestamps.

Deterministic tests cover:

- workspace normalization and input bounds;
- missing-decision order;
- default materialization;
- stable key derivation;
- byte-identical repeated compilation;
- exact requirement ordering;
- coverage completeness;
- manifest and hash stability;
- unknown recipe and version rejection;
- duplicate feature and generated-key conflicts;
- unsupported raw template and permission rejection;
- plan normalization acceptance;
- atomic candidate rollback;
- existing validation and StudyRoom golden simulation.
- Help, Join, and optional any-member Close interaction traces;
- a dead-button rejection check for recipe output.

Gemma evaluation covers:

- complete StudyRoom one-shot;
- the same StudyRoom split across three to five turns;
- an incomplete private-room request that needs exactly one blocking decision;
- brainstorming followed by a build command;
- a copy-only follow-up edit;
- a conflicting target;
- unsupported XP, timer, economy, and event-time LLM requests;
- a custom flow that correctly falls back to the typed planner;
- restart while waiting for a decision;
- failure with zero canonical Draft mutation.

The complete StudyRoom prompts either omit Close or explicitly request any-member Close. A creator-only Close prompt must produce a capability gap rather than an unsafe approximation.

Acceptance requires:

- compiler determinism 100%;
- recipe-path validate and simulate 100%;
- one-shot and multi-turn canonical intent and final structure equivalence 100%;
- explicit requirement coverage 100%;
- provenance from every generated requirement to one intent clause 100%;
- known-recipe selection at least 9 of 10 runs;
- complete one-shot unnecessary-question rate 0%;
- missing blocking-decision question rate 100%;
- unsupported silent degradation 0;
- failure-time canonical Draft mutations 0;
- repeated identical error loops 0;
- known-recipe RuleSet assembly model calls 0.

## Maintainability contract

- Each feature is an independent commit with a single responsibility.
- Every commit passes formatting, relevant focused tests, workspace tests, Clippy with warnings denied, and JavaScript evaluation checks when affected.
- Public API, serialization, snapshot, and error-code changes are explicit and tested.
- Code files remain responsibility-focused; orchestration, types, normalization, compilation, provenance, and routing are separate modules.
- No unchecked `serde_json::Value` crosses the typed recipe boundary.
- No recipe-specific conditional is added to the generic session loop when a catalog or protocol dispatch can own it.
- Golden fixtures and measurements are updated in the same commit as behavior they assert.
- Measurement reports distinguish deterministic compiler success from Gemma intent-extraction success.
- Failed experiments remain documented honestly and do not lower acceptance thresholds silently.

## Commit sequence

1. Design specification and acceptance contract.
2. Intent workspace types, normalization, deterministic decisions, and tests.
3. Stable keyspace, private StudyRoom recipe compiler, provenance, and deterministic tests.
4. Atomic compiled-plan execution adapter using existing plan and candidate gates.
5. Intent session protocol, snapshot persistence, and phase routing.
6. Gemma-only one-shot and multi-turn evaluation fixtures and reporting.
7. Measured prompt, router, and latency improvements.
8. Maintenance review, documentation, and final full-gate evidence.

Every commit must leave the branch green. The intent mode remains opt-in until the complete regression matrix and live Gemma threshold pass.
