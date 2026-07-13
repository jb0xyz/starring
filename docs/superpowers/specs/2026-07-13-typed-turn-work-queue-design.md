# Typed Turn Work Queue Design

## Decision

The harness will separate natural-language interpretation from deterministic Draft mutation.

The model proposes an exact typed turn plan. The harness compiles that plan into the existing design tools, applies the complete plan to a candidate Draft, verifies the requested postconditions, and commits only the complete candidate.

The engine, validator, simulator, activation boundary, and model gateway remain unchanged.

## Observed failure

The current model-facing `set_turn_brief` contract does not accept requirements. Parsed briefs always contain an empty requirements list.

An adaptive Build turn with no requirements falls back to every mutation tool that is legal for the current Draft. At the StudyRoom finalize stage this includes both declared-surface tools and runtime action tools. A valid but irrelevant `add_panel` call mutates the canonical Draft and satisfies the current changed-revision check even though it does not satisfy the human request.

The resource-stage measurement also showed that structural counts are insufficient. One run reached the expected panel, modal, rule, and action counts while using different resource names, unrelated channel references, and the wrong overwrite targets.

## Lifecycle

```text
Assess
→ Plan
→ Compile
→ CandidateExecute
→ ScopeCheck
→ Verify
→ Simulate
→ Preview
→ Reply
```

Brainstorm and inspection turns keep their existing direct discussion and verification paths. Typed planning applies to Build turns first. Modify, remove, and absence postconditions remain on the legacy adaptive path until the typed patch contract is implemented and measured.

## Plan contract

`TurnBrief.requirements` is the durable plan representation. It already serializes in session snapshots and expresses exact panels, buttons, modals, rules, actions, and unresolved-reference barriers.

The planning phase exposes only `set_turn_plan`. The input contains an ordered, non-empty list of `ScopeRequirement` values.

The plan validator enforces:

- at most 32 requirements;
- non-empty unique requirement identifiers;
- stable identity uniqueness;
- one shared created-action key namespace per rule across roles, channels, messages, and instances;
- rules before their actions;
- panels before their buttons;
- references to existing or earlier planned resources;
- keyed action minimum of one;
- positive absolute minimums for unkeyed actions;
- instance registration after all resources in its manifest;
- unresolved-reference barriers only at the end;
- no unsupported instance-role or created-instance teardown encoding;
- no activation, deployment, publication, Discord, or engine-store operation.

The model specifies semantic values, not internal tool names. The harness maps each requirement to one canonical design tool and complete canonical arguments.

## Execution

The complete plan executes on one clone of the canonical Draft. The pre-plan Draft remains available as the transaction root until every requested validation, simulation, and preview phase succeeds.

Each requirement is reconciled before dispatch:

- an exact existing keyed value is already satisfied;
- a divergent value with the same stable identity is a plan conflict;
- an absent value compiles to a canonical tool call;
- an unkeyed action executes only until its absolute minimum is reached.

Every compiled call uses the existing `dispatch_tool` path. The final candidate must satisfy all exact plan requirements. Only then does the harness replace the canonical Draft.

The harness also derives the action order produced by the existing insertion rules and verifies the final candidate against that accepted merged order. Tool-side placement of defer, registration, and response-edit actions cannot silently change the declared plan semantics.

Any parse, compile, dispatch, conflict, postcondition, validation, simulation, or preview failure discards or rolls back the candidate. A wrong plan therefore cannot increment the externally retained Draft revision or leave duplicates.

Runtime PostPanel instance actions are provisional until registration. The compiler permits the existing pending instance reference only when the same plan contains a later matching registration. Final scope checking occurs after registration has normalized those references.

## Routing

The intended routing formula is:

```text
exposed tools
= lifecycle phase tools
∩ typed plan tools
∩ Draft-legal tools
```

Before a plan is accepted, only `set_turn_plan` is exposed. Candidate execution is automatic and does not ask the model to repeat values already present in the plan. While a plan is pending, `check_turn_scope`, `finish_turn`, and broad mutation tools are unavailable.

After candidate commit, scope completion and requested gates are automatic. The model receives `finish_turn` only in Reply.

## Recovery

The model receives one structured opportunity to replace an invalid plan. A failed requested gate restores the pre-plan Draft, clears the rejected requirements, and routes only `set_turn_plan`. A second planning, execution, or gate failure halts the logical user turn. Typed-plan failure never falls back to the broad mutation registry.

Model or schema failures are reported as resumable harness failures. Only missing product decisions become human questions.

## Persistence

Accepted requirements remain in the existing adaptive turn snapshot. The prompt anchor retains the current exact requirements, so later operations do not depend on the truncated human-intent summary.

The first implementation performs candidate execution synchronously after plan acceptance. There is no partially committed internal frontier to recover. Later packet execution must add checkpoint events at plan acceptance and packet commit without adding SQLite to the library.

## Rollout

Typed planning is initially opt-in through a planned session constructor and evaluation input mode. The existing adaptive constructor remains unchanged for modification and removal regressions.

The rollout sequence is:

1. exact oracle plan with isolated StudyRoom resource and finalize fixtures;
2. model-authored plan with the same fixtures;
3. five-turn incremental StudyRoom;
4. one-user-turn StudyRoom with internal planning;
5. simple, clarification, additive, and replacement regression matrix;
6. typed patch and removal contract;
7. planned mode as the default adaptive path after the regression matrix is green.

## Measurement contract

The evaluation report records plan source, injected control calls, planned requirement count, compiled tool calls, candidate commits, plan conflicts, plan failures, semantic stage result, and exact final gate stamps.

The first acceptance threshold is:

- isolated resource stage exact semantics in three of three runs;
- isolated finalize stage exact semantics in three of three runs;
- incremental turn-five reach in three of three runs;
- incremental validate and golden simulation in at least two of three runs;
- zero wrong-scope canonical mutations;
- zero duplicate stable identities;
- no regression in existing deterministic gates.

After smoke acceptance, the product threshold is at least nine exact completions in ten production-plan runs.

## Model decision

Gemma and Qwen must be compared from the same commit, context, gateway contract, plan mode, fixtures, and run order.

Oracle-plan results separate execution capability from plan extraction:

- Gemma oracle success with production-plan failure indicates a planning contract problem;
- Gemma oracle failure with Qwen oracle success indicates a probable model ceiling;
- both oracle failures indicate compiler, schema, or argument burden;
- both oracle successes with broad-router failures indicate that model replacement is unnecessary.

Oracle evaluation is fail-closed. An injected plan may not delegate a replacement plan to the live model, and a passing oracle sample requires exact injection, submission, acceptance, commit, and zero-failure provenance counts.

Legacy Qwen reports used older schemas and do not decide this comparison.
