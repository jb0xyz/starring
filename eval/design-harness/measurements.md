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

## 5. Multi-turn StudyRoom experiment

Measured 2026-07-13 after adding adaptive multi-turn lifecycle support and an unchanged-Draft verification path. The model was already resident on the GPU before the sample, and the preceding `additive_revision` regression passed once in 673,634 ms; that regression is excluded from the three-run sample below. Promptfoo cache was disabled, concurrency was one, and the provider timeout was supplied through `STARRING_EVAL_TIMEOUT_MS=3600000`. The API key and gateway base URL were supplied only through the process environment and Keychain. Raw reports remain in the ignored `results/` directory.

| Case | Runs | Pass rate | Completion rate | Validation rate | Required simulation rate | Mean ms | p95 ms | Mean model calls | Mean tool calls | Mean distinct mutation tools | Maximum identical error count |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| StudyRoom incremental | 3 | 0 | 0 | 0 | 0 | 1,248,852 | 1,663,373 | 31.33 | 31.67 | 7 | 2 |

All three reports were valid and none had a provider error. No run reached the fifth `validate-simulate` turn, so actual validation and actual StudyRoom simulation rates were both zero. The final cumulative Draft targets of one panel, one modal, two rules, eleven actions, ten distinct mutation tools, current validation, and current golden-trace simulation were not achieved.

The per-turn `New distinct` column reports mutation tool names first introduced during that turn. `Mutation calls` reports every mutation tool executed during the turn, including tools used in prior turns.

| Run | Turn | Outcome | Elapsed ms | Model calls | Tool calls | New distinct | Mutation calls | Draft after | Stall |
| ---: | --- | --- | ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | surface | progressed | 155,213 | 8 | 8 | 3: add_panel, add_button, add_modal | add_panel×1, add_button×1, add_modal×1 | p1/m1/r0/a0 | none |
| 1 | open-rule | progressed | 248,456 | 6 | 6 | 2: begin_rule, add_interaction_action | begin_rule×1, add_interaction_action×1 | p1/m1/r1/a1 | none |
| 1 | submit-resources | halted | 140,529 | 5 | 5 | 0 | begin_rule×1, add_interaction_action×1 | p1/m1/r2/a2 | `REPAIR_ATTEMPT_FAILED`: repaired `add_resource_action` still omitted required `key` |
| 1 | submit-finalize | not reached | n/a | n/a | n/a | n/a | n/a | n/a | prior turn halted |
| 1 | validate-simulate | not reached | n/a | n/a | n/a | n/a | n/a | n/a | prior turn halted |
| 2 | surface | progressed | 154,465 | 8 | 8 | 3: add_panel, add_button, add_modal | add_panel×1, add_button×1, add_modal×1 | p1/m1/r0/a0 | none |
| 2 | open-rule | progressed | 302,962 | 7 | 7 | 2: begin_rule, add_interaction_action | begin_rule×1, add_interaction_action×1 | p1/m1/r1/a1 | none |
| 2 | submit-resources | progressed | 560,580 | 12 | 12 | 3: add_resource_action, add_upsert_overwrite_action, add_grant_role_action | begin_rule×1, add_interaction_action×1, add_resource_action×2, add_upsert_overwrite_action×2, add_grant_role_action×1 | p1/m1/r2/a7 | none |
| 2 | submit-finalize | halted | 645,363 | 12 | 12 | 0 | add_panel×8, add_button×2 | p9/m1/r2/a7 | `MODEL_CALL_LIMIT_EXHAUSTED`: used top-level panel tools instead of `add_post_panel_action` |
| 2 | validate-simulate | not reached | n/a | n/a | n/a | n/a | n/a | n/a | prior turn halted |
| 3 | surface | progressed | 150,413 | 7 | 8 | 3: add_panel, add_button, add_modal | add_panel×1, add_button×1, add_modal×1 | p1/m1/r0/a0 | none |
| 3 | open-rule | progressed | 206,439 | 5 | 5 | 2: begin_rule, add_interaction_action | begin_rule×1, add_interaction_action×1 | p1/m1/r1/a1 | none |
| 3 | submit-resources | progressed | 534,800 | 12 | 12 | 3: add_resource_action, add_upsert_overwrite_action, add_grant_role_action | begin_rule×1, add_interaction_action×1, add_resource_action×2, add_upsert_overwrite_action×2, add_grant_role_action×1 | p1/m1/r2/a7 | none |
| 3 | submit-finalize | halted | 647,327 | 12 | 12 | 0 | add_panel×8, add_button×2 | p9/m1/r2/a7 | `MODEL_CALL_LIMIT_EXHAUSTED`: used top-level panel tools instead of `add_post_panel_action` |
| 3 | validate-simulate | not reached | n/a | n/a | n/a | n/a | n/a | n/a | prior turn halted |

The first two turns completed in all three runs. The seven-mutation third turn completed in two of three runs. Both runs that reached the fourth turn confused runtime panel actions with top-level panel construction and exhausted the per-turn model-call budget after adding duplicate panels. The first run failed earlier while repairing the first resource action. This locates two independent stalls: the seven-tool resource turn remains marginal, and the four-tool finalize turn needs a more discriminating tool route or a smaller split around runtime panel posting.

The prior one-shot `studyroom_full` sample in section 4 also had 0% pass and 0% completion across three runs. The five-turn `studyroom_incremental` experiment therefore did not improve complex StudyRoom completion above the known one-shot 0% result. It did make the stall boundary observable and allowed two runs to reach the correct cumulative structure through seven actions, but that is partial progress, not completion. The hypothesis that this exact five-turn decomposition makes complex StudyRoom viable on local Gemma 12B is rejected by this sample.
