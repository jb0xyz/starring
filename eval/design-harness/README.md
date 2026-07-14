# Design Harness Promptfoo benchmarks

This benchmark executes the real `design-harness-cli --eval-json` loop against the configured OpenAI-compatible gateway. It does not recreate the agent loop in JavaScript and it never reads or writes Discord, deployment, or production database state.

The two cohorts are intentionally separate.

- `promptfooconfig.yaml` and `cases.yaml` retain the historical adaptive and typed-plan benchmark, including its old oracle experiments.
- `promptfooconfig.intent.yaml` and `intent-cases.yaml` are the close-disabled private-room checkpoint cohort. They are fixed to `gemma4:12b-mlx`, a declared 16384-token benchmark context policy, concurrency one, no cache, and evaluation input schema 3.

Schema 3 never hydrates fixtures, never accepts `initial_draft`, and never injects `oracle_brief` or `oracle_plan`. Its original JSON is passed to Rust so duplicate fields remain detectable by strict deserialization. The provider supplies one strict resource-binding document through `STARRING_HARNESS_BINDINGS_JSON`; the CLI derives the model-facing channel catalog and every deterministic gate from that same binding map. The configured IDs are isolated evaluation identifiers and do not cause Discord access.

The CLI accepts a legacy plain-text prompt or a stateful document. Every scripted turn in a stateful document runs on the same `DesignSession`. `AwaitingHuman`, `NeedsInput`, `Progressed`, `Completed`, and `Ready`-like outcomes return control to the script so the next human turn can run. Only `Halted` stops the script early.

```json
{
  "schema_version": 1,
  "turns": [
    { "id": "idea", "input": "피드백 자동화를 만들고 싶어" },
    { "id": "details", "input": "긴 글 모달로 받고 감사 메시지를 보내줘" }
  ]
}
```

Input schema version 2 selects the typed-plan lifecycle. It can start from an exact serialized Draft and can inject optional deterministic `set_turn_brief` and `set_turn_plan` controls for an oracle experiment. An oracle plan requires a Build brief and is consumed strictly after that brief; a non-Build brief cannot carry an oracle plan. Fixture references are expanded by the provider before the document reaches the CLI. A configured control is injected only when it is the sole exposed tool. An unexpected frontier, repeated control, unconsumed control at turn end, or cross-turn reset makes the oracle turn fail closed. All unconfigured model calls still use the configured gateway. Passing oracle cases require exact control injection accounting and exact submitted, accepted, and committed plan provenance with no plan execution failure, rollback, or conflict.

```json
{
  "schema_version": 2,
  "mode": "typed_plan",
  "initial_draft": { "$fixture": "studyroom_before_resources" },
  "turns": [
    {
      "id": "resources",
      "input": "리소스 단계를 완성해줘",
      "oracle_brief": {
        "intent": "build",
        "objective": "Complete the resource stage",
        "requested_outcome": "draft_update",
        "assumptions": [],
        "validate": false
      },
      "oracle_plan": { "$fixture": "studyroom_resources_plan" }
    }
  ]
}
```

The version 2 report contains `turns`, cumulative observability, per-turn observability deltas, revision continuity, and elapsed time. Typed-plan observability separates submissions, accepted requirements, compiled tool calls, execution failures, rollbacks, commits, and conflicts. `actual_gates` reports stamps that the model really earned during the session. `postcheck` reports the evaluator's non-mutating validation and simulation checks. These are intentionally separate so a successful evaluator postcheck cannot be mistaken for a gate the model called.

The legacy cases cover a complete one-shot request, an ambiguous request that should ask one blocking question, multi-turn elaboration, additive and replacement revisions that must preserve earlier work, the complex StudyRoom one-shot stress test, and typed-plan StudyRoom one-shot, five-turn, resource, and finalize paths. Production typed-plan cases delegate every call to the configured model. Oracle cases inject only their declared controls and separately measure the remaining delegated model calls.

The Intent Recipe cases cover English and Korean one-shot builds, a two-turn missing-hub decision, a SQLite save and connection close/reopen while that decision is pending, discussion followed by build, typed-planner routing, creator-only and stateful-runtime capability gaps, and live-mutation and secret-disclosure rejection. The prompts are natural user requests. They do not contain tool calls, raw RuleSet JSON, compiled requirements, fixtures, or evaluator controls.

## Local checks

Run from the repository root:

```sh
PATH=/opt/homebrew/opt/rustup/bin:$PATH cargo build -p design-harness-cli
npm --prefix eval/design-harness ci
npm --prefix eval/design-harness run check
```

## Gemma4 Intent checkpoint

The live checkpoint must run from a clean committed source tree. Before every sample the provider invokes `cargo build --locked -p design-harness-cli`, forbids an alternate harness binary, hashes the executable, and requires the child to match that digest and its embedded build commit and dirty state. The report records the runner source, local unsigned build identity, opaque gateway identity, exact binding fingerprint, pinned harness budgets, run order, timestamps, requested model, verified served model, and declared context policy. The OpenAI-compatible gateway does not expose its active context window, so `gateway_context_observed_tokens` is deliberately null and 16384 is not described as a gateway observation.

```sh
mkdir -p eval/design-harness/results
TOOLCHAIN_BIN="$(dirname "$(rustup which cargo)")"
PATH="$TOOLCHAIN_BIN:$PATH" \
CARGO="$TOOLCHAIN_BIN/cargo" \
STARRING_LLM_BASE_URL=http://127.0.0.1:18080/v1 \
STARRING_LLM_API_KEY="$STARRING_LLM_API_KEY" \
npm --prefix eval/design-harness run eval:intent -- \
  --repeat 10 \
  --output results/gemma4-intent.json
npm --prefix eval/design-harness run summarize -- results/gemma4-intent.json
npm --prefix eval/design-harness run accept:intent -- results/gemma4-intent.json
```

The Intent provider always requests exactly `gemma4:12b-mlx`; changing `STARRING_LLM_MODEL` cannot create a mixed Intent cohort. The CLI independently rejects a different served model. The evaluation script fixes concurrency at one and disables cache so every repeat is a real model sample. Acceptance also requires non-overlapping run timestamps. Its 60-second process boundary prevents unbounded interactive calls.

`acceptance.js` requires the exact ten-case manifest, at least ten samples for every case, every Promptfoo component assertion to pass, at least 90% known-recipe selection per case, 100% gates and 22 compiled operations on selected close-disabled recipes, zero unnecessary questions for complete requests, 100% required-decision resolution, exact mutation-free fallback routing, one model and one frontier call per turn, oracle isolation, and per-case repeat stability. The English one-shot, confirmed multi-turn, restored multi-turn, and discussion-then-build forms must have identical semantic hash, compiled plan hash, and final RuleSet. The one-shot and confirmed multi-turn exact input hashes must differ, while the SQLite roundtrip must preserve the confirmed multi-turn input hash. Preview-turn latency must remain below 8 seconds at P50 and 20 seconds at P95, and every turn must stay within 60 seconds.

This is a checkpoint for the close-disabled default recipe slice, not a commercial-readiness certificate. It does not cover the supported any-member Close variant, custom copy and naming overrides, independent Intent v3 postchecks, live stale/conflict prompts, a separate-process restart, multi-writer load, throughput, compensation, or external Discord failure recovery. Those remain explicit follow-up work even if the checkpoint passes.

The summarizer never merges metadata silently. It reports model, declared context, whether gateway context was observed, opaque gateway identity, source and binary identities, binding fingerprint, run ID and order span, timestamp span, dirty-source rate, oracle-isolation rate, recipe-selection rate, deterministic operation counts, and latency percentiles. `metadata_mixed: true` or more than one metadata boundary invalidates the checkpoint.

Historical Qwen measurements remain in `measurements.md` as prior diagnostic evidence. They must not be added to, compared inside, or used to satisfy the Gemma4 Intent checkpoint.
