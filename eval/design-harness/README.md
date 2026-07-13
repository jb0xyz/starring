# Design Harness Promptfoo benchmark

This benchmark executes the real `design-harness-cli --eval-json` loop against the configured OpenAI-compatible gateway. It does not recreate the agent loop in JavaScript and it never reads or writes Discord, deployment, or production database state.

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

Input schema version 2 selects the typed-plan lifecycle. It can start from an exact serialized Draft and can inject one deterministic `set_turn_plan` call for an isolated oracle experiment. Fixture references are expanded by the provider before the document reaches the CLI. Oracle injection occurs only when `set_turn_plan` is the sole exposed tool; all other model calls still use the configured gateway. If an injected plan is rejected, the oracle turn fails closed instead of delegating a replacement plan to the gateway. Passing oracle cases require exact injected, submitted, accepted, and committed plan provenance with no plan execution failure, rollback, or conflict.

```json
{
  "schema_version": 2,
  "mode": "typed_plan",
  "initial_draft": { "$fixture": "studyroom_before_resources" },
  "turns": [
    {
      "id": "resources",
      "input": "리소스 단계를 완성해줘",
      "oracle_plan": { "$fixture": "studyroom_resources_plan" }
    }
  ]
}
```

The version 2 report contains `turns`, cumulative observability, per-turn observability deltas, revision continuity, and elapsed time. Typed-plan observability separates submissions, accepted requirements, compiled tool calls, execution failures, rollbacks, commits, and conflicts. `actual_gates` reports stamps that the model really earned during the session. `postcheck` reports the evaluator's non-mutating validation and simulation checks. These are intentionally separate so a successful evaluator postcheck cannot be mistaken for a gate the model called.

The cases cover a complete one-shot request, an ambiguous request that should ask one blocking question, multi-turn elaboration, additive and replacement revisions that must preserve earlier work, the complex StudyRoom one-shot stress test, and typed-plan StudyRoom one-shot, five-turn, resource, and finalize paths. Production typed-plan cases delegate every call to the configured model. Oracle cases inject only the declared plan calls and separately measure the remaining delegated model calls.

Run from the repository root:

```sh
PATH=/opt/homebrew/opt/rustup/bin:$PATH cargo build -p design-harness-cli
npm --prefix eval/design-harness ci
mkdir -p eval/design-harness/results
STARRING_LLM_BASE_URL=http://127.0.0.1:18080/v1 \
STARRING_LLM_API_KEY="$STARRING_LLM_API_KEY" \
STARRING_LLM_MODEL=gemma4:12b-mlx \
npm --prefix eval/design-harness run eval -- \
  --output results/gemma-stateful.json
npm --prefix eval/design-harness run summarize -- results/gemma-stateful.json
```

Run Qwen only against a gateway that is actually configured to serve `qwen3.5:9b-mlx`. Changing `STARRING_LLM_MODEL` changes the requested model and result identifier; it does not reconfigure a fixed-model gateway. Verify `/v1/models` before every sample. Keep concurrency at one for the current single-model server and keep `--no-cache` so repeated runs measure the model.
