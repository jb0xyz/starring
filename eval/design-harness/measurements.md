# Serving measurements

All rows use Promptfoo cache disabled, concurrency one, the local OpenAI-compatible gateway at `127.0.0.1`, and `gemma4:12b-mlx`. Elapsed values are end-to-end harness burst time. Raw reports are kept locally under the ignored `results/` directory.

## Baseline

Measured 2026-07-13 before serving improvements 1–4. The requested and gateway-reported model were both `gemma4:12b-mlx`; the model context was 16K. The harness used a 16,000-character context budget, 12 model calls, 24 executed tool calls, and four gate failures. The Rust baseline binary was built from `b84d2a6`. One earlier run per case warmed the resident model and was excluded; the table aggregates the next three runs.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom full | 3 | 0 | 0 | 0 | 0 | 89,949 | 94,787 | 12 | 11.33 | 5 | 1 |
| Simple modal acknowledgement | 3 | 0 | 0 | 0.33 | n/a | 71,330 | 74,197 | 12 | 12 | 2 | 8 |

The StudyRoom runs halted with `UNSTRUCTURED_MODEL_TEXT` or `MODEL_CALL_LIMIT_EXHAUSTED`, reaching only one or two actions. All simple runs exhausted the model-call limit; one validated an incomplete zero-action Draft and two created duplicate modals. The same malformed interaction-action arguments recurred up to eight times.
