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

The version 2 report contains `turns`, cumulative observability, per-turn observability deltas, revision continuity, and elapsed time. `actual_gates` reports stamps that the model really earned during the session. `postcheck` reports the evaluator's non-mutating validation and simulation checks. These are intentionally separate so a successful evaluator postcheck cannot be mistaken for a gate the model called.

The cases cover a complete one-shot request, an ambiguous request that should ask one blocking question, multi-turn elaboration, additive and replacement revisions that must preserve earlier work, and the complex StudyRoom one-shot stress test.

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

Run Qwen against the gateway that serves it by changing `STARRING_LLM_BASE_URL` and `STARRING_LLM_MODEL=qwen3.5:9b-mlx`. The provider no longer overrides `STARRING_LLM_MODEL`, and its identifier includes the selected model. Keep concurrency at one for the current single-model server and keep `--no-cache` so repeated runs measure the model.
