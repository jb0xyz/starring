# Design Harness Promptfoo benchmark

This benchmark executes the real `design-harness-cli --eval-json` loop against the configured OpenAI-compatible gateway. It does not recreate the agent loop in JavaScript and it never reads or writes Discord, deployment, or production database state.

The full StudyRoom case requires both existing gates. The small modal case requires final validation only because the current simulator is deliberately a StudyRoom golden trace, not a general RuleSet simulator.

Run from the repository root:

```sh
PATH=/opt/homebrew/opt/rustup/bin:$PATH cargo build -p design-harness-cli
npm --prefix eval/design-harness ci
mkdir -p eval/design-harness/results
STARRING_LLM_BASE_URL=http://127.0.0.1:18080/v1 \
STARRING_LLM_API_KEY="$STARRING_LLM_API_KEY" \
npm --prefix eval/design-harness run eval -- \
  --repeat 3 \
  --output results/run.json \
  --output results/run.html
npm --prefix eval/design-harness run summarize -- results/run.json
```

`STARRING_LLM_MODEL` defaults to `gemma4:12b-mlx`. A provider entry can set another model for comparison when its gateway honors the requested model. Keep concurrency at one for the current single-model server and keep `--no-cache` so repeated runs measure Gemma.

The JSON report includes terminal outcome, final gate results, exact RuleSet semantics, Draft shape, distinct mutation tools, model and tool calls, repeated error signatures, and elapsed time. The command disables Promptfoo telemetry and its hidden result store; requested raw output stays under the ignored `results/` directory.
