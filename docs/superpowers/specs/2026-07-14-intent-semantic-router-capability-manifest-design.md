# Intent Semantic Router and Capability Manifest Design

## Decision

Intent recipe protocol V2 uses one uniform semantic-interpretation tool per ordinary human turn. Gemma extracts a bounded high-level IR, while deterministic harness code chooses the route and decides whether every hard requirement is supported.

```text
human turn
→ Gemma interpret_intent_turn
→ strict typed semantic IR
→ derive capability requirements
→ deterministic capability manifest adjudication
→ reject | discussion | capability gap | typed planner | exact recipe
→ existing deterministic recipe compiler and gates
```

The model no longer chooses `private_study_room`, `typed_planner`, `capability_gap`, `reject`, or `discussion` directly. It also cannot author capability identifiers, support statuses, manifest digests, recipe identity, low-level requirements, RuleSet JSON, or deployment operations.

The serving model remains exactly `gemma4:12b-mlx`. The normal contract remains one model call and one model tool call per human turn. Recipe expansion, compilation, validation, simulation, and preview remain model-free.

## Evidence for the change

The clean main baseline at commit `27969b0fb5d3edf970f856882cace1e384b268b2` ran all ten intent cases ten times. Known private-room recipes selected, compiled, validated, and simulated successfully, but all thirty route-boundary samples failed in three stable ways:

- custom static feedback halted ten of ten times after the monolithic route schema mixed an irrelevant private-room proposal into the typed-planner route;
- creator-only close compiled ten of ten times as close-disabled, silently weakening the explicit authorization requirement;
- the stateful game entered the private-room hub decision ten of ten times instead of reporting unavailable runtime capabilities.

The cause was the single flat `route_intent_turn` schema: a small route enum plus one large optional private-room proposal and several unrelated optional fallback payloads. Route-specific tools removed the direct ambiguity in a nine-call diagnostic, but they lose the gateway's single-tool JSON-schema constraint and JSON-content promotion.

A uniform semantic IR diagnostic preserved the single-tool gateway path and produced fifteen native tool calls from fifteen requests. The derived deterministic route was correct in all fifteen samples: three complete private rooms, three custom feedback flows, three creator-only rooms, three stateful games, and three direct-live-mutation requests. Latency was P50 6.727 seconds and P95 14.216 seconds, below the existing 8-second and 20-second targets. The diagnostic also showed that model-authored fallback prose could contradict the derived capability decision, so V2 does not expose that prose as authoritative output.

These diagnostics are design evidence, not acceptance evidence. Commercial claims still require the committed implementation, full deterministic gates, and a clean repeated cohort from one source commit.

## Uniform semantic IR

The only ordinary-turn frontier is `interpret_intent_turn`. Its schema contains no route field and no route-specific union.

```text
expected_revision
request_mode
automation_kind
objective
requested_outcome
hub_channel?
locale
close_authorization
runtime_requirements
boundary_requests
unclassified_requirements
response_locale
response
copy?
naming?
controls?
```

The required classifier fields are closed enums.

```text
request_mode:
  discussion | build

automation_kind:
  managed_private_study_room | custom_automation | none

requested_outcome:
  discussion | working_draft | validated_preview

locale and response_locale:
  en | ko | unspecified

close_authorization:
  not_requested | disabled | any_member | creator_only

runtime_requirements.persistence:
  none | restart_persistent

runtime_requirements.timers:
  none | durable

runtime_requirements.economy:
  none | persistent_ledger

runtime_requirements.event_time_llm:
  boolean

boundary_requests:
  direct_live_mutation
  bypass_validation_preview_approval
  secret_disclosure
```

`unclassified_requirements` contains only hard runtime, authorization, lifecycle, external-effect, or policy requirements not represented by the closed fields. Static panels, buttons, modals, messages, and deterministic actions that the existing typed planner can express do not belong there. The list is bounded, normalized, and fail-closed.

The private-room copy, naming, and control text fields preserve the V1 recipe customization surface. `close_authorization` replaces the model-facing close policy. `creator_only` is a requested semantic that can be preserved and rejected before compilation; it is never inserted into the compiler's supported `ClosePolicyV1` enum. `disabled`, `any_member`, and an unspecified close request convert deterministically to the existing proposal type only after capability adjudication succeeds.

The schema excludes schema versions, durable workspace revisions other than `expected_revision`, feature IDs, recipe IDs, provenance, source classifications, permission bits, generated keys, instance manifests, raw actions, raw templates, RuleSet JSON, Discord identifiers, publication, approval, deployment, and activation.

## Parse and consistency contract

The parser rejects malformed JSON, duplicate fields, unknown fields, unknown enums, oversized values, and invalid binding keys before adjudication. All route-driving fields are typed before session code sees them.

The following consistency rules are deterministic:

- `discussion` mode requires `requested_outcome=discussion`;
- `build` mode requires `working_draft` or `validated_preview`;
- `automation_kind=none` is valid for discussion and boundary-violation requests, but it cannot enter recipe compilation;
- a build with `automation_kind=none` and no boundary or capability finding is an inconsistent interpretation and halts without mutation;
- a managed private-room build is the only interpretation that can be converted to `PrivateStudyRoomProposalV1`;
- private-room override fields on a non-recipe route have no authority and never mutate the Draft;
- a selected channel key must exist in the harness-owned binding map before recipe preparation;
- nonempty unclassified requirements prevent recipe compilation;
- no fallback response supplied by the model can change a route, blocker, support status, or user-visible capability fact.

The model provides a bounded `response` so a discussion turn can remain a natural AI conversation. A discussion requires a nonempty response. Build turns are instructed to use an empty response to reduce output latency, although any bounded build response is accepted and ignored. The harness surfaces model prose only when deterministic adjudication selects `discussion`. The same field has no authority on build, typed-planner, capability-gap, or reject routes. The harness owns those fallback facts and renders their final response from deterministic templates. The live diagnostic produced capability-gap responses that incorrectly promised to build the unsupported request, so V2 never surfaces model prose for those outcomes.

## Capability manifest

The capability manifest is a pure, statically linked catalog inside `design-harness`. It has no LLM, session, SQLite, network, Discord, publication, approval, or activation dependency.

Initial user-contract capability identifiers are:

| Capability ID | Status | Route effect |
|---|---|---|
| `instance_creator_teardown_authorization` | `unavailable` | capability gap |
| `restart_persistent_state` | `unavailable` | capability gap |
| `durable_timer` | `unavailable` | capability gap |
| `persistent_economy_ledger` | `unavailable` | capability gap |
| `event_time_llm_decision` | `forbidden_policy` | capability gap with policy status |
| `unclassified_intent_requirement` | `unclassified` | capability gap |

These identifiers describe user-visible contracts, not Rust types or implementation mechanisms. A capability descriptor contains the ID, status, route effect, and deterministic English and Korean labels. The manifest is sorted by capability ID and has a versioned SHA-256 digest over one canonical serialization. Descriptor order, duplicate IDs, unknown IDs, status drift, and digest drift are tested.

The event-time LLM descriptor records the stable policy identifier `event_time_llm_execution_forbidden_v1`. Policy identity is part of the manifest digest and route decision.

`unclassified` means that the current closed authoring catalog cannot establish support for the preserved hard requirement. It is a safe capability-gap outcome, not an assertion that the requirement is permanently unavailable and not an internal parser failure.

Safety boundaries are a separate immutable catalog because they always reject rather than describe runtime availability.

| Safety boundary ID | Route effect |
|---|---|
| `direct_live_mutation` | reject |
| `bypass_validation_preview_approval` | reject |
| `secret_disclosure` | reject |

The interpreter's typed slots derive capability requirements. The model never supplies status or route effect.

```text
close_authorization=creator_only
→ instance_creator_teardown_authorization

persistence=restart_persistent
→ restart_persistent_state

timers=durable
→ durable_timer

economy=persistent_ledger
→ persistent_economy_ledger

event_time_llm=true
→ event_time_llm_decision

each boundary request
→ matching policy capability

unclassified_requirements nonempty
→ unclassified_intent_requirement
```

Derived requirements are deduplicated and sorted. A supported requirement cannot be reported as a gap, and an unavailable or forbidden requirement cannot enter compilation.

Normalization produces an opaque validated semantic IR. Adjudication is the only constructor of opaque recipe and typed-planner route permits. Each permit is bound to the canonical semantic-IR digest and current capability-manifest digest, and lowering consumes the permit with the same values. Session code cannot construct a private-room proposal first and ask the adjudicator afterward.

## Deterministic route precedence

Adjudication uses the following order:

```text
explicit boundary violation
→ reject

discussion request without an explicit boundary violation
→ discussion

build with unavailable, forbidden runtime, authorization, or unclassified requirement
→ capability gap

supported managed private-room build
→ exact pinned recipe

other supported static build
→ typed planner
```

Discussion precedes runtime capability-gap reporting because brainstorming about an unsupported idea should remain a natural, mutation-free conversation. An explicit request to cross the live-mutation, safeguard, or secret boundary is still rejected.

A mixed build with any blocker never compiles a supported subset. The harness can discuss alternatives, but a downgrade or partial build requires a later explicit user turn and a new interpretation.

## Deterministic route decision

Every routed turn records a structured decision owned by the harness.

```text
kind
decision_source=deterministic_intent_adjudicator
semantic_ir_digest
manifest_version
manifest_digest
adjudication_digest
blockers[]
unclassified_requirements[]
```

Each blocker records the exact capability ID and status. Recipe and typed-planner routes record an empty blocker list. The decision is exposed in the intent evaluation report and public routed outcome without adding it to the RuleSet or durable recipe workspace.

Build-route fallback wording is rendered deterministically from the decision and response locale. It identifies unsupported or forbidden requirements without promising compilation. Typed-planner wording says that the custom static design is routed to the typed planner. Reject wording names the preserved design, approval, and secret boundary. A discussion route may surface the bounded model response because it cannot mutate the Draft and carries no capability authority. The model remains useful for semantic extraction and brainstorming, but it is not the authority for capability facts.

## Protocol and snapshot versioning

`INTENT_RECIPE_PROTOCOL_VERSION` changes from 1 to 2. `SESSION_SNAPSHOT_VERSION` remains unchanged because the outer snapshot structure does not change.

New sessions use the V2 system prompt and `interpret_intent_turn` frontier. The existing `resolve_intent_decision` frontier remains the sole tool while a deterministic channel decision is pending.

V1 snapshots are not silently reinterpreted under the V2 prompt. Snapshot validation recognizes the exact V1 prompt only far enough to return an explicit unsupported intent-protocol error instructing the caller to start a new intent session. A V1 protocol number with a V2 prompt, or a V2 protocol number with a V1 prompt, fails as an invariant violation. This pre-commercial checkpoint does not migrate transcript semantics in place.

The existing public V1 route parser may remain available for source compatibility, but the V2 session never exposes or executes it. Legacy compatibility code must not weaken the V2 model frontier.

## Session and safety invariants

The existing atomic boundary remains unchanged.

- Parse or adjudication failure leaves the Draft byte-equivalent.
- Reject, discussion, capability-gap, and typed-planner outcomes perform zero deterministic recipe operations and leave the Draft byte-equivalent.
- Capability adjudication runs before private-room workspace preparation, missing-decision creation, compilation, or candidate execution.
- A blocked private-room request cannot ask for a hub channel.
- A blocked request records zero compile attempts and zero commits.
- The exact recipe path still compiles, atomically executes, validates, simulates, previews, and commits through the existing pipeline.
- Event-time LLM remains forbidden by the existing engine guards; the manifest describes that boundary but does not replace it.
- No live Discord, database, publication, approval, deployment, or activation capability is added.

## Evaluation changes

The existing ten-case cohort remains the first regression denominator. It gains exact assertions for every routed turn:

- deterministic decision source;
- valid manifest digest and version;
- exact route kind;
- exact blocker ID and status sets for creator-only and stateful requests;
- zero blockers for typed-planner, discussion, and recipe paths;
- zero compile attempts, deterministic operations, and Draft mutation for fallback routes;
- no unsupported model-authored promise in the surfaced response.

Expected creator-only blockers:

```text
instance_creator_teardown_authorization: unavailable
```

Expected stateful-game blockers:

```text
restart_persistent_state: unavailable
durable_timer: unavailable
persistent_economy_ledger: unavailable
event_time_llm_decision: forbidden_policy
```

After the existing cohort passes, a separate contrastive cohort covers:

- creator-only close versus any-member close;
- persistent versus explicitly stateless automation;
- durable timer versus no timer;
- event-time LLM decision versus design-time AI assistance;
- direct live mutation versus a request to produce a safe design;
- English and Korean variants;
- negation and mixed supported-plus-unsupported requests;
- a novel hard requirement that must enter `unclassified_requirements` and fail closed.

The clean repeated acceptance run must still use one exact source commit, one clean binary, one Gemma model tag, one context declaration, no alternate harness, no result mutation, and no retry-merging. The existing thresholds remain unchanged: known-recipe selection at least 9/10, exact fallback behavior 100%, recipe validation and simulation 100%, unnecessary questions zero, one model/tool call per ordinary turn, P50 below 8 seconds, P95 below 20 seconds, and every turn below the 60-second safe boundary.

## Commit sequence

1. Fix structural evaluation equality and add Promptfoo VM regression coverage.
2. Commit this V2 design and measured diagnostic decision.
3. Add the pure capability manifest, canonical digest, adjudicator, and focused tests.
4. Add the strict uniform semantic IR parser and conversion tests without changing the active session.
5. Switch the intent session to protocol V2, add deterministic route responses, and explicitly reject V1 snapshots.
6. Add structured route-decision reporting and exact evaluation assertions.
7. Run focused Rust and JavaScript tests, workspace tests, Clippy with warnings denied, formatting, and dependency guards.
8. Run the five-case live smoke from one clean commit.
9. Run the full ten-case ten-repeat cohort only after the smoke passes.
10. Add contrastive cases, record honest measurements, and continue measured improvements if any threshold fails.

Each feature commit remains green and contains one responsibility. Rust source changes add no comments. Engine, runtime safety, publication, approval, deployment, and activation crates remain unchanged.
