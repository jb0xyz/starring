# Serving measurements

All rows use Promptfoo cache disabled, concurrency one, the local OpenAI-compatible gateway at `127.0.0.1`, and `gemma4:12b-mlx`. Elapsed values are end-to-end harness burst time. Raw reports are kept locally under the ignored `results/` directory.

## Baseline

Measured 2026-07-13 before serving improvements 1–4. The requested and gateway-reported model were both `gemma4:12b-mlx`; the model context was 16K. The harness used a 16,000-character context budget, 12 model calls, 24 executed tool calls, and four gate failures. The Rust baseline binary was built from `b84d2a6`. One earlier run per case warmed the resident model and was excluded; the table aggregates the next three runs.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom full | 3 | 0 | 0 | 0 | 0 | 89,949 | 94,787 | 12 | 11.33 | 5 | 1 |
| Simple modal acknowledgement | 3 | 0 | 0 | 0.33 | n/a | 71,330 | 74,197 | 12 | 12 | 2 | 8 |

The StudyRoom runs halted with `UNSTRUCTURED_MODEL_TEXT` or `MODEL_CALL_LIMIT_EXHAUSTED`, reaching only one or two actions. All simple runs exhausted the model-call limit; one validated an incomplete zero-action Draft and two created duplicate modals. The same malformed interaction-action arguments recurred up to eight times.

## 1. Cache-friendly Prompt Builder

Measured 2026-07-13 after moving the changing Draft anchor to the request tail, making canonical messages append-only, and trimming only an outbound copy. Conditions and three-run sample size match the baseline.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom full | 3 | 0 | 0 | 0.67 | 0 | 77,575 | 90,272 | 12 | 11.67 | 4.67 | 2 |
| Simple modal acknowledgement | 3 | 0 | 0 | 1 | n/a | 66,373 | 89,192 | 11 | 10.67 | 2.33 | 8 |

Mean latency fell 13.8% for StudyRoom and 7.0% for the simple case. Two StudyRoom runs reached a validator-clean but incomplete Draft, and one simple run reached the exact two-action Draft and human-question terminal state. All evaluation assertions still failed because tool confusion and call budgets remain unresolved.

## 2. Deterministic Tool Router

Measured 2026-07-13 after routing the locked registry by Draft dependencies. An empty Draft exposes three tools; buttons, role grants, instance registration, validation, and simulation become available only when their deterministic prerequisites hold. Hidden calls are rejected before dispatch and batch calls are rechecked against the current Draft. Conditions and sample size match the baseline.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom full | 3 | 0 | 0 | 0 | 0 | 74,981 | 87,429 | 12 | 12 | 4.67 | 2 |
| Simple modal acknowledgement | 3 | 0 | 0 | 1 | n/a | 71,410 | 82,596 | 12 | 12 | 2 | 7 |

StudyRoom mean latency was 3.3% lower than Item 1 and 16.6% lower than baseline. The simple case was effectively unchanged from baseline and 7.6% slower than Item 1. No run completed: StudyRoom reached at most one action, while every simple run repeated the same malformed interaction-action shape seven times. The router removes impossible choices and lifecycle deadlocks, but the live result shows that structured one-attempt repair is still required.

## 3. SQLite state compression

Measured 2026-07-13 after adding versioned session snapshots, CLI-edge SQLite persistence, a structured Draft anchor, bounded human-intent memory, and outbound-only compaction after the 16,000-character budget is exceeded. Conditions and sample size match the baseline. Persistence and restart recovery are covered by deterministic close/reopen and continuation tests; this single-burst live benchmark does not exercise a process restart.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom full | 3 | 0 | 0 | 0.33 | 0 | 111,769 | 131,074 | 12 | 12 | 4.33 | 5 |
| Simple modal acknowledgement | 3 | 0 | 0 | 0.67 | n/a | 113,560 | 116,813 | 12 | 12 | 2.33 | 8 |

The durable state and compact overflow path pass their deterministic tests, but this short-session live benchmark regressed: mean latency rose 49.1% for StudyRoom and 59.0% for the simple case relative to Item 2. The richer append-only anchors remain in context while the transcript is below budget, and repeated malformed calls still consume all 12 model calls. One StudyRoom run reached a validator-clean incomplete Draft, while one simple run failed validation. These results make Item 4's bounded repair path necessary; Item 3 alone is not a live latency or completion improvement.

## 4. Validator one-attempt repair

Measured 2026-07-13 after adding a persisted one-attempt repair state machine. Argument errors expose the same tool and exact schema once; validation repair permits one mutation followed by exact validation; simulation repair adds exact simulation after validation. Malformed, mismatched, or repeatedly failing repair responses halt immediately. Full repair tickets stay in SQLite snapshots while the prompt anchor carries a bounded repair summary. Conditions and final three-run sample size match the baseline; an earlier one-run smoke check was excluded.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count | Mean repair attempts | Mean repair successes | Mean repair failures |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom full | 3 | 0 | 0 | 0.33 | 0 | 185,738 | 214,993 | 11 | 10.33 | 4.67 | 2 | 4.33 | 4 | 0.33 |
| Simple modal acknowledgement | 3 | 0 | 0 | 1 | n/a | 177,277 | 223,919 | 10 | 9.33 | 3.33 | 1 | 4.33 | 3.33 | 1 |

The repeated-error contract improved decisively: the worst identical error count fell from 5 to 2 for StudyRoom and from 8 to 1 for the simple case, and no simple run repeated an error. Mean model calls fell 8.3% and 16.7% respectively relative to Item 3, and all simple Drafts passed final validation. Completion and exact-task pass rates remained zero. Latency still rose 66.2% for StudyRoom and 56.1% for the simple case because the model produced several different malformed calls instead of one repeated signature, averaging 4.33 repair attempts per run. Item 4 prevents unbounded flailing per error but does not impose a session-wide repair budget; that is the next performance policy to evaluate rather than an unmeasured claim of improvement.
