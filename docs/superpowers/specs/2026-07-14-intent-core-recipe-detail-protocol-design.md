# Intent Core and Recipe Detail Protocol Design

## Decision

Intent recipe protocol V3 separates high-level routing semantics from recipe-specific presentation details.

```text
human turn
→ interpret_intent_core
→ strict Core IR
→ deterministic capability adjudication and recipe selection
   ├─ default recipe configuration → deterministic compile
   └─ explicit recipe customization → extract_private_study_room_details
                                      → deterministic coverage check
                                      → deterministic compile
→ existing validate, simulate, preview boundary
```

The serving model remains exactly `gemma4:12b-mlx`. The model never selects a recipe identifier, authors a capability status, writes RuleSet JSON, or invokes deployment or live Discord operations.

Default StudyRoom, custom static fallback, discussion, capability-gap, and reject turns use one model call and one model tool call. A supported StudyRoom with explicit copy, naming, or control customization may use exactly one additional model call and one additional model tool call. There is no model repair or retry.

## Measured reason for the split

The first committed V2 smoke used five prompts with three fixed-seed repetitions. All fifteen turns halted before deterministic adjudication:

- three complete StudyRoom calls emitted the unknown field `language`;
- three custom static calls emitted a type-invalid object;
- three creator-only calls returned prose instead of a tool call;
- three stateful-game calls emitted a misplaced `persistence` field;
- three direct-live-mutation calls returned prose instead of a tool call.

The Draft remained unchanged, and the adjudicator and compiler ran zero times. The result measured wire adherence, not semantic routing quality.

One raw custom-static response classified the request correctly as `custom_automation`, but used `null` for enum fields, empty runtime requirements, and `null` for optional recipe objects. The current model-facing schema is 4,839 bytes, and duplicating it in `tools` and `response_format` creates 10,403 bytes of structured metadata.

An earlier fifteen-call diagnostic used a 1,461-byte twelve-field schema and 3,493 bytes of structured metadata. All fifteen responses arrived as native tool calls, and deterministic route derivation was correct for all fifteen. The diagnostic did not include recipe copy, naming, controls, or a separate response locale.

Two alternative transport experiments are rejected:

- the local gateway intentionally rejects `tool_choice=required`;
- response-format-only generation returned fenced arbitrary JSON that did not match even a small schema.

V3 therefore keeps the proven native tool channel with `tool_choice=auto` and reduces the active frontier schema.

## Core IR

The ordinary-turn frontier is `interpret_intent_core`.

```text
expected_revision
request_mode
automation_kind
objective
requested_outcome
hub_channel
locale
close_authorization
runtime_requirements
boundary_requests
unclassified_requirements
detail_facets
response
```

The first twelve fields retain the successful diagnostic semantics. `detail_facets` is the only addition. It is a bounded array containing at most three closed facets.

```text
detail_facets[]:
  copy | naming | controls
```

The original human message remains the authority for the exact custom literals passed to the recipe-specific extractor. The model cannot put behavior, permissions, actions, identifiers, manifests, or raw templates in `detail_facets`. Those are either represented by the closed Core IR, routed to the typed planner, or preserved as an unclassified hard requirement.

`hub_channel` retains the field name that produced native tool calls in the fifteen-call diagnostic. The selected recipe adapter owns its meaning. A future recipe that needs multiple binding roles must use its own detail extractor rather than growing the Core IR.

`locale` controls deterministic default recipe copy and deterministic fallback wording. V3 removes the redundant model-facing `response_locale`. The model `response` is surfaced only for discussion; it has no authority on build, capability, recipe, or rejection facts.

## Core normalization

The parser rejects malformed JSON, duplicate or unknown fields, unknown enums, invalid binding keys, oversized values, and inconsistent mode and outcome combinations.

Normalization:

- trims and bounds the objective, response, and requirements;
- sorts and deduplicates boundary and unclassified requirements;
- sorts and deduplicates the bounded detail facets;
- rejects recipe details on discussion and boundary-only requests;
- rejects build requests with `automation_kind=none` unless a boundary or capability finding explains the terminal route;
- treats explicit default copy or naming as no customization requirement;
- never drops a hard runtime, authorization, lifecycle, external-effect, or safety-boundary requirement.

Schema size is a tested product invariant. The first generated V3 Core schema is 2,397 bytes and its combined `tools` plus `response_format` structured metadata is 5,285 bytes. The schema must remain at or below 2,400 bytes and combined metadata at or below 5,600 bytes. A field addition requires an explicit design and updated measured budget.

## Deterministic adjudication

The V2 capability manifest and route precedence remain authoritative.

```text
explicit safety boundary → reject
discussion → discussion
unsupported hard requirement → capability gap
supported managed StudyRoom → pinned StudyRoom recipe
other supported static automation → typed planner
```

Capability and safety adjudication runs before recipe-detail extraction. A creator-only, persistent-state, durable-timer, persistent-economy, event-time-LLM, unclassified, or boundary-violating request cannot spend a second model call and cannot compile a supported subset.

## Default fast path

A pinned StudyRoom uses deterministic defaults when `detail_facets` is empty.

The adapter constructs the existing `PrivateStudyRoomProposalV1` from:

- Core objective;
- requested outcome;
- selected hub channel;
- locale;
- supported close authorization;
- existing deterministic copy, naming, and control defaults.

The existing workspace preparation, compiler, validator, simulator, atomic candidate commit, and receipt remain unchanged. V3 must produce the same Draft, operation count, semantic plan, validation stamp, and simulation stamp as V2 for equivalent default input.

## Recipe detail path

Only a pinned StudyRoom with nonempty `detail_facets` exposes `extract_private_study_room_details`.

The harness retains the pinned recipe identity, Core semantic digest, and current revision outside the model frontier. The model receives the original human request and harness-owned detail facets. It cannot change route, recipe, requested outcome, selected binding, locale, authorization, runtime requirements, or safety boundaries.

The broad public V1 parser retains this compatibility shape:

```text
copy?
naming?
controls?
unmapped_facets[]?
```

Unrequested objects may be omitted. Each requested facet must have at least one normalized value in its matching object. A nonempty `unmapped_facets` list fails closed. After structural validation, the harness stamps the active revision, Core semantic digest, and canonical selected facets into the internal detail object. The model never authors binding or coverage metadata.

The active detail router derives one per-request schema from the canonical selected facets and reuses that same schema for exposure and serving-parser error translation. It removes every unselected object and `unmapped_facets`, then marks every remaining facet object as required. The serving parser requires the top-level argument keys to equal that selected set exactly, including rejection of empty unselected objects. An extractor that cannot map a selected facet must return its required object empty, which the parser rejects without mutation. The broad public parser schema remains compatible with omitted objects and explicit `unmapped_facets`, but the serving frontier exposes only the closed active subset.

The active serving V2 wire keeps only the selected `copy`, `naming`, and `controls` objects and flattens every pattern into sibling `*_prefix` and `*_suffix` string fields before converting it to the typed recipe pattern. It never asks the model to alternate between scalar literals and nested `{prefix,suffix}` objects. Supplying either nonempty affix creates the pattern and an omitted counterpart becomes the empty string. A present pattern with two empty affixes fails closed. The broad public parser retains the original nested typed pattern shape.

The optional second model request uses a bounded isolated context containing only the detail system prompt, the current `INTENT_HUMAN` envelope, and the current `INTENT_DETAIL_STATE` anchor. The append-only durable transcript still records both accepted tool calls and results, but the extractor does not receive the Core tool arguments, Core tool result, or unrelated prior conversation.

After normalization and facet validation, every nonempty scalar and pattern affix must be an exact case-sensitive contiguous substring of the current raw human turn. CRLF pairs are compared as LF so accepted multiline values remain grounded. The check does not search prior turns and runs before the detail digest, evidence creation, recipe finalization, preparation, or compilation. This proves literal membership, not semantic field attribution when the same literal legitimately appears in more than one place.

The detail result is rejected without mutation when:

- a selected facet is missing, duplicated in the harness ticket, unknown, empty, or mapped to another facet;
- an unmapped facet remains;
- a present pattern has no nonempty affix;
- a nonempty literal is absent from the current human turn;
- a value exceeds the existing recipe bounds;
- a control contradicts the adjudicated close authorization;
- the response changes a Core field or supplies an unsupported field;
- the call count would exceed two model calls or two model tool calls.

Defaults may fill unrequested fields. Defaults never fill a requested but missing detail.

The reduced StudyRoom detail schema is 2,024 bytes and must remain at or below 2,100 bytes.

## State and atomicity

The detail extraction stage is transient inside one burst. The durable stages remain empty, awaiting a user decision, and preview ready.

Before the first model call, the session records the stable Draft and intent stage. If the optional second call or detail validation fails, the canonical Draft and durable intent stage remain byte-equivalent to that stable state. The transcript may record the rejected model output and structured error, but no half-extracted workspace is persisted.

Successful awaiting-decision and preview-ready stages bind:

- protocol version;
- resource-binding fingerprint;
- canonical Core IR digest;
- deterministic route decision;
- pinned recipe and registry digest;
- extraction mode `deterministic_default` or `model_detail`;
- detail request, extracted detail, and coverage digests;
- typed workspace and current revision;
- compiler and candidate receipts for preview-ready state.

V1 and V2 intent snapshots are rejected with an explicit unsupported-protocol error. They are never reinterpreted under the V3 prompt.

## Recipe registry boundary

Recipe dispatch uses a closed enum, not runtime trait objects.

```text
RecipeKindV1:
  private_study_room_v1
```

A descriptor pins recipe ID and version, extractor revision, normalizer revision, compiler revision, simulator revision, and requirement bounds. The registry digest covers the canonically sorted complete descriptor list. A selected-recipe digest and the full-registry digest are distinct values.

Adding a recipe requires an exhaustive match arm for selection, default configuration, detail extraction, normalization, compilation, simulation, semantic projection, and external bindings. Missing registration must fail compilation or a deterministic registry test.

## Conversation and patching

Preview-ready anchors expose a compact harness-owned summary: recipe family, locale, selected bindings, close policy, non-default detail facets, workspace revision, and feature identity.

A later patch distinguishes `keep`, `reset_default`, and `set`. It recompiles the entire recipe-owned feature atomically while preserving stable feature and generated resource keys. The first V3 activation may keep patch authoring behind deterministic rejection until these three operations and their equivalence tests are implemented; it must not silently treat an absent value as reset.

## Evaluation

The first gate is the existing five-case smoke with one run each, then three runs each. The full ten-case ten-repeat cohort runs only after the smoke passes.

Acceptance retains:

- known-recipe selection at least 9/10 per case;
- exact fallback, blocker, and boundary behavior 100%;
- recipe validation and simulation 100%;
- unnecessary questions zero;
- default ordinary turns exactly one model call and one model tool call;
- explicit detail turns at most two model calls and two model tool calls;
- model repair and retry zero;
- P50 below 8 seconds, P95 below 20 seconds, and every call below 60 seconds.

A separate detail cohort tests exact custom literals for copy, naming, and controls, missing and duplicate coverage, creator-only precedence, and one-shot versus multi-turn equivalence. A later contrastive cohort tests negation, supported versus unsupported runtime requirements, safe design versus live mutation, Korean and English variants, and novel unclassified hard requirements.

## Implementation sequence

1. Commit this measured design decision.
2. Add the V3 Core wire, parser, digest, schema budget, and tests without activating it.
3. Add the closed recipe descriptor and default StudyRoom adapter without changing compiler output.
4. Add the private StudyRoom detail wire and pure coverage validator without activating it.
5. Activate protocol V3, reject V2 snapshots, and bind Core and extraction receipts.
6. Add evaluator evidence for frontier path, extraction mode, schema bytes, call counts, and digests.
7. Run deterministic gates and the clean Gemma smoke.
8. Run the full cohort only after smoke success, then add the detail and contrastive cohorts.

Every implementation commit remains green. Rust changes add no comments. Engine, runtime safety, publication, approval, deployment, activation, Discord, and database boundaries remain unchanged.
