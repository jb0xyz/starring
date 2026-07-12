# Harness MVP — Conversational Design Validation — Design

## Goal

Validate stage 2's riskiest unknown: **can an E2B / gemma-class home LLM actually
drive a multi-step conversational design through tools?** Concretely, a person
describes a StudyRoom-class automation in natural language; the model, with no
human tool-selection, calls several design tools across a bounded autonomous
burst to assemble a Draft `InteractionRuleSet`; the Draft passes the existing
structural and binding validation; and a representative golden-trace scenario
executes successfully through the deterministic runner. Everything is in-memory
and offline — no live Discord, no publish, no activation.

This is a validation slice, not a product. If the model cannot do this on the
current (②-level) engine, the full ③ vision is not worth building yet; if it can,
this seeds the real harness.

## Nature and Non-Goals

Not required, and explicitly out of scope:

- live Discord installation, RuleSet publish / activation, the approval flow;
- generalized "design any automation"; a finished chat UI; long-term memory /
  persistence; multi-agent; CI-scale eval;
- any change to the engine or the 18f safety boundary.

The MVP connects **Draft construction + validate + a golden-trace simulation**,
and nothing downstream of a Draft.

## Safety Guard

- The tool registry has **no `activate` / `deploy` / `publish` tool**. The model
  can only build a Draft.
- The Draft never touches `RuleSetStore`, the activation authority, Discord, or
  any persistent store. `design-harness` must not depend on `twilight`, `sqlx`,
  `automation-ruleset*`, or `automation-*-postgres`.
- The LLM gateway base URL and Bearer key come only from environment / the OS
  keychain at the edge. They must never appear in code, config, the system
  prompt, tests, or committed files.

## Context

The current engine (Layer 2) is complete and live-proven through 18f. This MVP
sits on top of it and reuses:

- `automation-state` — the `InteractionRuleSet` / `PanelSpec` / `ModalSpec` /
  `InteractionRule` / `TriggerSpec` / `ActionSpec` types the tools build.
- `automation-core` — `interpret`, `validate` (structural + bindings), and the
  deterministic `run()` executed against mock services (the existing Layer-2
  interaction test pattern). **The `simulator` crate is unrelated (not Layer 2)
  and is not used.**

The target model is `gemma4:e2b-mlx` behind the OpenAI-compatible gateway
(function tools ≤ 32; tool calls only with `stream=false`; the gateway runs no
agent loop — the harness runs it; small models sometimes emit prose instead of a
tool call).

## Architecture

Two units, split so the loop logic is testable without the live model.

```
crates/design-harness/   (library)
  draft        in-memory Draft + revision counters
  tools        11 model-facing tools: DTO input -> normalize -> automation-state builder -> Draft
  gates        validate_draft (automation-core validate) / simulate_draft (run() over mock services)
  errors       validate/run errors -> structured { code, location, message, hint }
  session      the agent loop + QUESTION/DONE protocol + nudge + serial exec + bounds + logging
  llm          LlmClient trait + LlmResponse { ToolCalls | Text }  (mockable)
  schema       JSON schema per tool DTO, for the LLM tools array

tools/design-harness/    (binary)
  a gemma HTTP LlmClient (OpenAI-compatible chat-completions)
  a terminal conversation (read human turns, run the session, print QUESTION/DONE/results)
  base URL + Bearer key from env / keychain
```

`design-harness` deps: `automation-state`, `automation-core`, `serde`,
`serde_json`, a JSON-schema deriver, `thiserror`. No `twilight` / `sqlx` /
ruleset / postgres.

## The Draft Model

An in-memory accumulator with three revision counters:

```rust
struct Draft {
    ruleset: InteractionRuleSet,
    draft_revision: u64,
    validated_revision: Option<u64>,
    simulated_revision: Option<u64>,
}
```

- Every successful mutation tool: `draft_revision += 1`; `validated_revision =
  None`; `simulated_revision = None`.
- `validate_draft` success: `validated_revision = Some(draft_revision)`.
- `simulate_draft` success: `simulated_revision = Some(draft_revision)`.
- Completion requires `simulated_revision == Some(draft_revision)`; a mutation
  after a passing simulation immediately invalidates it.

The Draft carries a fixed mock `ResourceBindingMap` (e.g. a `study_hub` channel)
so validate/simulate have a binding context without any live Discord.

## The Design Tools (11)

Model-facing tools take **model-friendly DTOs**, normalized inside Rust into
`ActionSpec` / spec types — the engine's serde/enum forms are never exposed, so
engine type changes do not break the tool contract. References use a small DTO
form: `{ "created": "<alias>" }` or `{ "existing": "<binding_key>" }`.

Structure:

- `add_panel(key, channel, content)`
- `add_button(panel_key, label, route)` — `route`: `{static: button_key}` or
  `{instance_action: action}`
- `add_modal(key, title, fields[])` — `field { key, label, style, required }`
- `begin_rule(key, trigger)` — `trigger`: `{button_click: component}` /
  `{modal_submit: modal}` / `{instance_action: action}`

Actions (appended to a rule), grouped by meaning so each schema stays small and
shallow — deliberately **not** one big `ActionSpec` union:

- `add_resource_action(rule_key, kind, key, name)` — `kind`: `create_role` /
  `create_channel`
- `add_permission_action(rule_key, kind, ...)` — `kind`: `upsert_overwrite`
  `{channel, target, allow[], deny[]}` (`target`: `everyone` / `{role: ref}`) or
  `grant_role` `{role: ref, target}` (`target`: `actor`)
- `add_interaction_action(rule_key, kind, ...)` — `kind`: `open_modal {modal}` /
  `defer_ephemeral` / `edit_response {content}`
- `add_post_panel_action(rule_key, key, channel, content, buttons[])`

Instance registration is its own tool (complete-footprint invariant, finalizing
order, nested manifest — never hidden in an action union):

- `set_register_instance(rule_key, instance_key, kind, roles[], channels[], messages[])`
  where each manifest entry names a created alias.

Gates:

- `validate_draft()` — runs `automation-core::validate` (structural + bindings).
- `simulate_draft()` — see The Golden Trace.

No `finish` tool (the model declares `DONE:`); no `describe_draft` tool (every
mutation result carries a short state summary instead, so the model never has to
decide when to describe).

## Tool Result Format

Short and structured, never the whole Draft. Success:

```json
{
  "ok": true,
  "revision": 7,
  "change": "Added CreateChannel action to rule submit_room",
  "draft": { "panels": 2, "modals": 1, "rules": 3, "actions": 11,
             "unresolved_references": ["hub_join_panel"] },
  "validation": "stale",
  "simulation": "stale"
}
```

Failure — a translated, machine-usable form (never raw Rust debug):

```json
{
  "ok": false,
  "code": "UNRESOLVED_CREATED_REFERENCE",
  "location": "rule.submit_room.actions[4]",
  "message": "Created channel study_channel does not exist yet",
  "hint": "Add create_channel before this action or correct the alias",
  "revision": 7
}
```

Error codes (small models use `code + location + hint` more reliably than prose):
`INSTANCE_RESOURCE_MISSING`, `REGISTER_BEFORE_RESOURCE_CREATED`,
`UNRESOLVED_CREATED_REFERENCE`, `DRAFT_NOT_VALIDATED`, and a bounded set covering
the existing `ValidationError` variants, each mapped to `{code, location,
message, hint}`.

## The Agent Loop and Text Protocol

One human message opens an **autonomous burst**: multiple model calls and tool
executions run without per-tool human approval.

```
human message -> conversation
loop (within bounds):
  call model (stream=false, tools = the 11 schemas, parallel_tool_calls=false)
  tool_calls present -> execute in return order (serial); re-inject each result
                        (with its auto-summary); continue the burst
  text only -> parse the text protocol:
      "QUESTION: <q>" -> show to human, end burst, wait for the next human message
      "DONE: <summary>" -> if simulated_revision == draft_revision -> complete;
                           else internal nudge, continue burst
      other prose -> internal nudge once; if still prose next turn, show it to the
                     human and halt (stop auto-driving)
  bounds exceeded -> halt and report
```

Text-only replies are restricted by the system prompt to exactly `QUESTION:` or
`DONE:`. The nudge: "Call a design tool to change the Draft; use QUESTION: to ask
the human; only use DONE: after simulate_draft passes on the current revision."

**Serial execution** is mandatory (the Draft is one mutable state):
`parallel_tool_calls=false` when the gateway supports it; regardless, the harness
executes returned calls in order. A failing call sets the remaining calls to
`NotExecutedAfterPreviousFailure` and re-injects the failure plus the latest
Draft summary — the model never proceeds on assumed state.

**Validate-before-simulate:** `simulate_draft` on an unvalidated revision fails
with `DRAFT_NOT_VALIDATED` (hint: validate first). This makes the model learn the
lifecycle: design → validate → fix → validate → simulate → DONE.

## Bounds and Logging

Fixed per design session (a human clarification answer does **not** reset the
budget; nudge re-calls count as model calls):

```
max model calls:               12
max executed tool calls:       24
max validate+simulate failures: 4
```

On exceeding any bound: halt immediately, **no fixture fallback**, print the
current Draft summary, the last validation/simulation error, and which limit was
exhausted.

Per-session observability log: `model_calls`, `tool_calls`,
`distinct_mutation_tools`, `clarification_count`, `validation_failures`,
`simulation_failures`, `nudge_count`.

## LLM Integration

**Deployment.** The home server (Mac mini) only exposes the model as an
OpenAI-compatible API (Node gateway → Ollama → `gemma4:e2b-mlx`), already
reachable over a Cloudflare Tunnel. `design-harness` is a **client** of that API,
built in this repo like all other Starring code and run on the developer machine
(or anywhere with network access and the key). It is **not** deployed to the home
server; there is no home-server build/agent/CI for it.

The gemma edge (`LlmClient` impl) calls chat-completions with `model =
gemma4:e2b-mlx` (the gateway forces this model and ignores the field, but send it
for clarity), `messages`, `tools` = the 11 DTO JSON schemas, `tool_choice = auto`
(the gateway supports only `auto` / `none`), `stream = false` (tool calls require
it). `parallel_tool_calls = false` when accepted; the harness serializes returned
calls regardless.

**Config, edge-only.** Base URL from env (default
`https://llm-api.starring.co.kr/v1`, overridable to the local
`http://127.0.0.1:18080/v1`) and the Bearer key from env / OS keychain on the
running machine. Both are read once at the edge and **never** appear in code,
config, the system prompt, tests, logs, or committed files.

**Gateway limits that shape the design.** Single concurrent generation (serial
execution already fits); per-request timeout 300s; max output 2048 tokens.

**Context budget (critical).** The configured context is **8192 tokens** and the
gateway caps input at **16000 characters**. Every request re-sends the system
prompt plus all 11 tool schemas plus the running conversation, so the harness
must stay within that budget: keep tool schemas compact; keep tool results short
(already the design); and, as a burst grows, **trim older tool-result messages**
while keeping the system prompt and a single running Draft-state summary as the
anchor. If a request would exceed the input cap, the harness trims oldest-first;
if it still cannot fit, it halts the burst and reports rather than sending a
truncated request. This budget interacts with the 12-model-call bound — a burst
cannot grow unboundedly.

The `LlmClient` trait returns `LlmResponse::ToolCalls(...)` or
`LlmResponse::Text(...)`, so `session` is driven by a scripted mock client in unit
tests and by the gemma HTTP client at the edge.

## The Golden Trace

`simulate_draft` executes one representative scenario through the deterministic
runner (not the `simulator` crate):

```
create button click            -> open study modal
modal submit (room_name)       -> create role, create private channel,
                                  @everyone deny view, member-role allow view,
                                  grant member role to the submitter,
                                  post welcome panel in the channel,
                                  post join panel in the hub,
                                  register the instance
```

Mechanism: `interpret` the triggered rule into an `ActionPlan`, then
`automation-core::run()` against **mock services** (in-memory mutation recorder,
responder, instance store, id generator, teardown) — the pattern the existing
Layer-2 interaction tests use — capturing the recorded mock mutations. Minimum
assertions:

```
exactly one role created; exactly one channel created;
GrantRole target == the submitting user;
a private (@everyone deny view) overwrite exists;
both panels' button routes resolve;
the RegisterInstance manifest is complete (every created ownable resource present once).
```

A `simulate_draft` pass means the designed automation actually runs and produces
the intended footprint — not merely that the JSON typechecks.

## System Prompt (draft)

States: the role (design Discord automations by calling tools, never touch live
Discord); the tool-use discipline (one change per tool call; reference created
resources by alias); the lifecycle (validate before simulate; only `DONE:` after
simulate passes); and the two allowed text forms (`QUESTION:` / `DONE:`). No
secrets, no engine internals.

## Success Criterion

A run counts as validating the unknown when all hold:

```
- a real human ↔ gemma conversation
- at least two DISTINCT mutation tools used (validate_draft / simulate_draft do not count)
- the same in-memory Draft repaired incrementally (no full regeneration, no template swap)
- latest-revision structural validation passes
- latest-revision binding validation passes
- latest-revision golden-trace simulation passes
- the model declares DONE:
- the human never edits JSON or the Draft
- no fixture / StudyRoom template was injected
- zero Discord mutation
- completed within all bounds
```

Self-repair is expected and counts as success: a first Draft that fails
validation, is read by the model, and fixed via tools to a passing state is the
target behavior, not a failure.

## Testing

- `session` loop logic: unit-tested with a **scripted mock `LlmClient`** —
  deterministic sequences exercising serial execution, mid-batch failure,
  QUESTION/DONE parsing, nudge-once-then-halt, revision invalidation,
  validate-before-simulate, and every bound. No gemma, no network.
- tools / gates: unit tests that each DTO normalizes to the right `ActionSpec`
  and that validate/simulate reflect the golden trace and its failure modes.
- the validation run itself is **live** (real human + gemma), recorded with the
  observability log — this is the experiment, not an automated test.

## ③-Extensibility Note

The `DTO -> builder -> Draft` shape and the structured-error protocol generalize:
③ later adds new action-DTO tools (state, condition, timer, session) and new
spec types without reworking the loop, the revision model, or the error contract.
The MVP builds only ②-level rulesets.

## Known Limitations

- One target model (gemma), no escalation to a larger model; degradation is
  "halt and report," not "retry on a bigger model."
- One golden trace, not exhaustive behavioral coverage.
- A minimal terminal conversation, not a product UI.
- The manual run validates capability, not reproducibility; the scripted eval
  (auxiliary regression on a fixed transcript) is a later addition.

## Roadmap

If the run succeeds: build the real harness (persistent `design-draft`,
richer tools, preview, then the safe landing path through the existing gates) and
begin the ③ stateful-runtime arc. If it fails: the vision needs a larger model or
a different tool shape before the runtime investment — which is exactly what this
slice is meant to reveal cheaply.

## Handoff

Spec-only, two chunks: **(1)** the `design-harness` library (Draft, tools,
gates over automation-core, error translation, the session loop generic over
`LlmClient`, mock-client tests); **(2)** the gemma HTTP `LlmClient`, tool-schema
generation, and the terminal binary.
