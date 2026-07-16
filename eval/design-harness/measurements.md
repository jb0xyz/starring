# Serving measurements

Sections 1–8 use Promptfoo cache disabled, concurrency one, and the local OpenAI-compatible gateway at `127.0.0.1` unless a row says otherwise. The baseline through section 5 uses `gemma4:12b-mlx`; section 6 names the model for every live row. Section 9 uses the native Luna-medium Codex worker. Elapsed values are end-to-end harness burst time unless labeled as Promptfoo wall time. Raw reports are kept locally under the ignored `results/` directory.

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
| 2 | submit-resources | progressed | 560,580 | 12 | 12 | 3: add_resource_action, add_upsert_overwrite_action, add_grant_role_action | begin_rule×1, add_interaction_action×1, add_resource_action×2, add_upsert_overwrite_action×2, add_grant_role_action×1 | p1/m1/r2/a7 | structural count reached, but role/channel names and both overwrites differed from the requested semantics |
| 2 | submit-finalize | halted | 645,363 | 12 | 12 | 0 | add_panel×8, add_button×2 | p9/m1/r2/a7 | `MODEL_CALL_LIMIT_EXHAUSTED`: used top-level panel tools instead of `add_post_panel_action` |
| 2 | validate-simulate | not reached | n/a | n/a | n/a | n/a | n/a | n/a | prior turn halted |
| 3 | surface | progressed | 150,413 | 7 | 8 | 3: add_panel, add_button, add_modal | add_panel×1, add_button×1, add_modal×1 | p1/m1/r0/a0 | none |
| 3 | open-rule | progressed | 206,439 | 5 | 5 | 2: begin_rule, add_interaction_action | begin_rule×1, add_interaction_action×1 | p1/m1/r1/a1 | none |
| 3 | submit-resources | progressed | 534,800 | 12 | 12 | 3: add_resource_action, add_upsert_overwrite_action, add_grant_role_action | begin_rule×1, add_interaction_action×1, add_resource_action×2, add_upsert_overwrite_action×2, add_grant_role_action×1 | p1/m1/r2/a7 | none |
| 3 | submit-finalize | halted | 647,327 | 12 | 12 | 0 | add_panel×8, add_button×2 | p9/m1/r2/a7 | `MODEL_CALL_LIMIT_EXHAUSTED`: used top-level panel tools instead of `add_post_panel_action` |
| 3 | validate-simulate | not reached | n/a | n/a | n/a | n/a | n/a | n/a | prior turn halted |

The first two turns completed in all three runs. The seven-mutation third turn returned `progressed` in two of three runs, but only run 3 matched the requested StudyRoom resource semantics. Run 2 used different role and channel names, referenced unrelated channels in both overwrites, and targeted everyone twice. Both runs that reached the fourth turn confused runtime panel actions with top-level panel construction and exhausted the per-turn model-call budget after adding duplicate panels. The first run failed earlier while repairing the first resource action. This locates three failures: the resource turn can halt, it can silently satisfy only the structural counts with wrong semantics, and the finalize turn needs an intent-bound tool route rather than broad Draft-state availability.

The prior one-shot `studyroom_full` sample in section 4 also had 0% pass and 0% completion across three runs. The five-turn `studyroom_incremental` experiment therefore did not improve complex StudyRoom completion above the known one-shot 0% result. It made the stall boundary observable, but only one run reached the exact requested cumulative semantics through seven actions. The other structurally complete resource turn was semantically wrong. This is partial progress, not completion. The hypothesis that this exact five-turn decomposition makes complex StudyRoom viable on local Gemma 12B is rejected by this sample.

## 6. Typed turn work queue

Measured 2026-07-14 while developing the typed outline → packet → independent review → atomic commit path. Every checkpoint row below is one diagnostic run against a different harness revision. These are development checkpoints, not independent repetitions, so their latency and pass/fail values must not be treated as reliability rates or causal performance comparisons. Promptfoo cache was disabled and concurrency was one. Elapsed values below are harness-reported end-to-end time.

| Checkpoint | Scope and outcome | Harness ms | Model/tool calls | Canonical revision | Commits/rollbacks | Raw report |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Strict structured reviewer | Surface halted because the reviewer returned a natural object shape that failed the exact field type contract | 97,754 | 6/6 | 0→0 | 0/0 | `results/gemma-incremental-typed-structural-final.json` |
| Tolerant reviewer parser | Surface committed; open-rule then halted because the model did not echo every exact reference-audit token | 252,200 | 11/14 | 0→3 | 1/0 | `results/gemma-incremental-typed-tolerant-review.json` |
| Evidence verdict reviewer | Surface and open-rule committed; the resource packet halted after malformed packet fields and a prose response | 983,353 | 24/27 | 0→5 | 2/0 | `results/gemma-incremental-typed-reference-verdict.json` |
| Normalized resource packets | Isolated resource turn passed, producing the exact resource prefix with five distinct mutation tools | 331,494 | 11/17 | 5→12 | 1/0 | `results/gemma-isolated-resources-normalized-packets.json` |
| Reference-verdict finalize | Isolated finalize turn passed, adding four actions with exact panel targets and derived instance manifest | 306,048 | 10/13 | 12→16 | 1/0 | `results/gemma-isolated-finalize-reference-verdict.json` |
| First full normalized run | Surface, open-rule, and resources committed; finalize halted before mutation because the initial candidate omitted its instance registration | 1,467,301 | 30/41 | 0→12 | 3/0 | `results/gemma-incremental-typed-normalized-full.json` |
| Response-format-only transport | Isolated resources halted at the brief frontier because Gemma returned empty text twice when the sole tool was removed from the upstream request | 25,733 | 2/0 | 5→5 | 0/0 | `results/gemma-isolated-resources-response-format-only.json` |
| Optional overwrite sides | Isolated resources passed after empty allow or deny lists became safely omittable, avoiding the observed Gemma `<nil>` tool-call serialization | 323,135 | 10/17 | 5→12 | 1/0 | `results/gemma-isolated-resources-optional-overwrite.json` |
| First hardened full run | All five turns were injected; four mutation turns committed, then final validation found a malformed channel-name template and its one repair call failed | 1,356,560 | 29/44 | 0→15 | 4/0 | `results/gemma-incremental-typed-hardened-full.json` |
| Template and new-rule hardening | Surface committed a panel and modal without the requested button; open-rule then repeated an unknown button reference and halted | 444,410 | 13/14 | 0→2 | 1/0 | `results/gemma-incremental-typed-template-hardened-full.json` |
| Operation inventory and semantic reference replan | Surface committed the exact panel, button, and modal; open-rule then halted after two reviewer sentinel field-type errors | 288,294 | 11/14 | 0→3 | 1/0 | `results/gemma-incremental-typed-inventory-replan-full.json` |
| Natural reviewer sentinel normalization | All five turns completed with exact StudyRoom semantics and current validation and simulation stamps | 2,060,168 | 38/56 | 0→16 | 4/0 | `results/gemma-incremental-typed-review-normalized-full.json` |
| Follow-up edge hardening rerun | Surface again committed exactly; open-rule then halted after two review responses failed the exact seven-field shape | 289,467 | 11/14 | 0→3 | 1/0 | `results/gemma-incremental-typed-edge-hardened-full.json` |
| Four-field review contract rerun | Surface committed exactly through the reduced review contract; open-rule semantic replan then submitted the same invalid post-panel owner twice | 298,446 | 11/14 | 0→3 | 1/0 | `results/gemma-incremental-typed-review-discriminated-full.json` |
| Four-field review Qwen rerun | The exact first three prefixes committed; finalize halted after schema-invalid packet-fill calls and a terminal response without exactly one packet-fill call | 1,092,274 | 29/40 | 0→12 | 3/0 | `results/qwen35-9b-incremental-typed-review-discriminated-full.json` |
| Reviewer-isolated Ornith rerun | Surface committed the panel and modal but omitted the requested button; open-rule then repeated the unknown button reference and halted | 236,033 | 11/13 | 0→2 | 1/0 | `results/ornith-9b-incremental-typed-review-isolated-full.json` |

The isolated resource and finalize results are each one pass from one run. They establish that those paths can succeed, but they do not meet the planned 3/3 reliability threshold.

The response-format-only transport experiment tried to bypass Ollama's Gemma tool-call parser by sending only the sole frontier's strict JSON schema and adapting JSON content back into the routed call. The model returned empty content on both attempts before planning began. The experiment was rejected and the upstream `tools` contract was restored.

Making the empty permission side optional removed the exact `<nil>` pressure without weakening overwrite semantics: both sides empty and overlapping permissions are rejected deterministically. The isolated resource turn then produced the exact rev5→12 prefix with seven compiled mutations and no Ollama tool-call parsing warning. This is one pass, not a reliability rate.

The next full run reached all five turns. Its revision path was `0→3→5→12→15→15`: resource construction succeeded, while finalize omitted `edit_response` and the resource turn had already committed the malformed channel template `study-${input.room_name`. Final validation reported `BAD_TEMPLATE`; the single repair attempted `update_action` with invalid arguments and halted. Canonical revision 15 was preserved. This moves the observed stall from packet serialization to an earlier missing template-syntax check plus one omitted finalize action; it is still a failed full run.

Template syntax and modal-input availability are now checked while typed packets are assembled, and every newly created rule must carry at least one same-candidate action. The next diagnostic run exposed a separate semantic boundary: the surface reviewer approved a candidate that omitted the requested button, and the following turn repeatedly referenced that nonexistent button. Adding an atomic operation inventory made the surface review exact in the subsequent run, while a full-candidate semantic reference failure now discards the assembly and spends the one semantic replan instead of retrying an impossible packet frontier. That run stopped at the next transport boundary because a no-issue reviewer response used natural `null` and object values for required string sentinels.

At that checkpoint, the production parser required the same exact seven review field names and rejected legacy shapes before normalizing values. For `none` or `missing`, natural `null` issue identifiers and an object-valued empty expected JSON were converted to the advertised internal sentinels. Mismatch evidence remained strict about candidate id, JSON Pointer, different value, and equal JSON type. With that normalization, one full incremental run reached the exact StudyRoom target and passed every Promptfoo assertion.

### First complete typed run

| Turn | Outcome | Elapsed ms | Model/tool calls | Revision | Commit/rollback | Draft after | Stall |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| surface | progressed | 114,019 | 6/9 | 0→3 | 1/0 | p1/m1/r0/a0 | none |
| open-rule | progressed | 179,143 | 5/7 | 3→5 | 1/0 | p1/m1/r1/a1 | none |
| submit-resources | progressed | 593,459 | 12/19 | 5→12 | 1/0 | p1/m1/r2/a7 | none |
| submit-finalize | progressed | 923,783 | 12/15 | 12→16 | 1/0 | p1/m1/r2/a11 | none |
| validate-simulate | ready | 249,757 | 3/6 | 16→16 | 0/0 | p1/m1/r2/a11 | none |

The final Draft had one panel, one modal, two rules, eleven actions, no unresolved references, and all ten required distinct mutation tools. Both actual gate stamps were current at revision 16, and the independent postchecks passed validation and the golden-trace StudyRoom simulation. The run used 38 model calls and 56 tool calls, made four atomic commits with no rollback, and stayed within the configured per-turn and total evaluation budgets. Its maximum identical error count was two.

This is the first observed complete complex typed run, so it establishes feasibility for local `gemma4:12b-mlx`; it does not establish reliability. It took 2,060,168 harness milliseconds and 34 minutes 21 seconds wall time. The slowest turn was `submit-finalize` at 923,783 ms, and later model calls reached roughly 80–88 seconds as context accumulated. This latency is not acceptable for an interactive commercial path without further work. The live binary for this checkpoint preceded the follow-up deterministic edge hardening for explicit extra-mutation review, independent reviewer/structural extension flags, and prior-template-dependency replan escalation; those changes are covered by deterministic tests rather than this live row.

The immediate rerun after those three edge changes did not reproduce the full completion. Surface again committed the exact rev0→3 structure in 121,800 ms, but open-rule produced two responses that did not contain the exact seven advertised review fields and halted with `PLAN_REPAIR_FAILED`. The canonical Draft stayed at revision 3; no validation or simulation stamp was current. Across these two adjacent but not identical harness checkpoints, complex completion is one observed pass and one observed fail. They are not a same-revision 1/2 reliability sample, but the failed rerun confirms that reviewer serialization remains a live reliability boundary. The follow-up contract therefore keeps one review tool but requires only `covered_ids`, `reference_verdict`, `issue_kind`, and `detail`; mismatch and extra evidence fields are conditionally required, while the old seven-field sentinel form remains compatible. The next diagnostic row exercises that contract.

The first live run with the four-field contract passed the surface review and again committed the exact panel, button, and modal at revision 3. The open-rule turn then referenced a nonexistent component, correctly triggered the one full-candidate semantic replan, and used both replacement attempts on a `post_panel` outline whose owner `submit_room` was not an existing rule. It halted before review with `INVALID_TOOL_ARGUMENTS@tool.set_turn_plan.arguments.steps.0.owner`; canonical revision 3 was preserved. This run clears the previous review-shape boundary but fails at a different Gemma planning boundary. Repeatedly changing the review interface cannot resolve that model variance.

The Qwen rerun used the same four-field contract and the then-current deterministic harness revision. It committed the exact surface, open-modal rule, and resource prefix along `0→3→5→12`, then halted during `submit-finalize`. The finalize flow produced schema-invalid packet-fill calls before a terminal response failed the contract requiring exactly one `fill_turn_plan_packet` call, so no finalize mutation executed and canonical revision 12 remained intact. The run stopped after four of five turns with `PLAN_REPAIR_FAILED`, used 29 model calls and 40 tool calls, and took 1,092,274 harness milliseconds. Validation correctly reported the still-incomplete `submit_room` rule and simulation was not attempted. This is a deeper failed trace than the adjacent Gemma rerun, not a completion or a reliability estimate. The subsequent reviewer-state prompt-injection isolation is covered by deterministic tests and was not exercised by this live row.

The Ornith run exercised the reviewer-state isolation on `ornith:9b`, a Qwen 3.5-family 9B Q4_K_M model, with the gateway capped to the same 16K context as the other samples. Its surface candidate contained the requested panel and modal but omitted the requested static button; the independent reviewer accepted that incomplete operation inventory and committed revision `0→2`. The open-rule turn then referenced the absent `create_study_room` button twice and halted with `PLAN_REPAIR_FAILED`, preserving revision 2. No actual validation or simulation stamp was current. The evaluator's non-mutating validation passed the structurally valid but incomplete Draft, while the golden-trace postcheck correctly failed with `GOLDEN_TRACE_OPEN_BUTTON_MISSING`. The run took 236,033 milliseconds with 11 model calls and 13 tool calls. It was faster only because it stopped after two turns, so this sample is not evidence of a latency advantage.

### First full-run boundary

| Turn | Outcome | Elapsed ms | Model/tool calls | Revision | Commit/rollback | Draft after | Stall |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| surface | progressed | 112,138 | 6/9 | 0→3 | 1/0 | p1/m1/r0/a0 | none |
| open-rule | progressed | 188,057 | 5/7 | 3→5 | 1/0 | p1/m1/r1/a1 | none |
| submit-resources | progressed | 607,123 | 12/19 | 5→12 | 1/0 | p1/m1/r2/a7 | none |
| submit-finalize | halted | 559,978 | 7/6 | 12→12 | 0/0 | p1/m1/r2/a7 | `PLAN_REPAIR_FAILED` after two `TURN_PLAN_INSTANCE_OWNER_AMBIGUOUS` errors |

The first three turns produced the exact cumulative resource prefix and committed atomically. In the fourth turn, the candidate contained instance-routed `post_panel` actions but initially omitted `register_instance`. Owner resolution ran before the coverage reviewer and treated the zero-registration case as ambiguous. It returned `TURN_PLAN_INSTANCE_OWNER_AMBIGUOUS` twice, the one semantic replan failed, and the canonical Draft remained unchanged at revision 12. No validation or simulation was attempted; the postcheck correctly reported `DEFER_MISSING_EDIT`. This run therefore did not complete StudyRoom.

### Structural coverage extension

The resolver now gathers every rule whose instance-routed panel lacks a registration. Multiple registrations in one rule remain `TURN_PLAN_INSTANCE_OWNER_AMBIGUOUS`. The harness retains the typed candidate and packet cursor, then exposes one deterministic extension that accepts exactly one `register_instance` for every listed owner and rejects missing, duplicate, extra-owner, or unrelated operations. A session-level guard prevents any second coverage extension in the same human turn. Owner resolution, manifest derivation, independent review, and atomic execution run only after the obligation is satisfied.

The deterministic regressions cover both one missing owner and two missing owners in the same extension. The two-owner path is:

`set_turn_brief → set_turn_plan → fill_turn_plan_packet ×2 → set_turn_plan(extension) → fill_turn_plan_packet(extension) → review_turn_plan → finish_turn`

It ends at revision 6 with two rules, two `post_panel` actions, and two `register_instance` actions in canonical order, with two plan submissions, one acceptance, one commit, zero rollbacks, and one nudge. The second-incomplete-review regression halts with the canonical revision unchanged. These results prove the deterministic harness route; they are not live-model evidence.

### Live model comparison

| Model | Raw report | Runs | Full result | Final revision and structure | Gates | Harness ms | Model/tool calls | Commits/rollbacks |
| --- | --- | ---: | --- | --- | --- | ---: | ---: | ---: |
| `gemma4:12b-mlx` | `results/gemma-incremental-typed-structural-extension-final.json` | 1 | evaluator fail; halted at `submit-resources` 3/5 with `PLAN_REPAIR_FAILED` | rev5; p1/m1/r1/a1 | validation not current; simulation not current | 866,742 | 22/25 | 2/0 |
| `qwen3.5:9b-mlx` | `results/qwen35-9b-incremental-typed-structural-extension-final.json` | 1 | evaluator fail; halted at `submit-resources` 3/5 with `PLAN_REPAIR_FAILED` | rev4; p1/m1/r1/a0 | validation not current; simulation not current | 649,885 | 22/26 | 2/0 |
| `gemma4:12b-mlx` | `results/gemma-incremental-typed-review-discriminated-full.json` | 1 | evaluator fail; halted at `open-rule` 2/5 after two invalid replacement outlines | rev3; p1/m1/r0/a0 | validation not current; simulation not current | 298,446 | 11/14 | 1/0 |
| `qwen3.5:9b-mlx` | `results/qwen35-9b-incremental-typed-review-discriminated-full.json` | 1 | evaluator fail; halted at `submit-finalize` 4/5 after packet schema errors and a terminal response-shape error | rev12; p1/m1/r2/a7 | validation not current; simulation not current | 1,092,274 | 29/40 | 3/0 |
| `ornith:9b` | `results/ornith-9b-incremental-typed-review-isolated-full.json` | 1 | evaluator fail; halted at `open-rule` 2/5 after repeating an unknown button reference | rev2; p1/m1/r0/a0 | validation not current; simulation not current | 236,033 | 11/13 | 1/0 |

Gemma committed the exact first two prefixes and preserved rev5 when the resource packet failed. The packet boundary and field-type errors were refined, but the last two Ollama responses contained malformed Gemma tool-call arguments and arrived at the harness as empty text. This run never reached the structural instance-registration extension, so it is not evidence that the extension failed.

Qwen also committed the first two turns, but its open-rule candidate omitted the requested `open_modal` action and the independent reviewer accepted that incomplete candidate. During the resource turn the reviewer twice reported mismatch evidence whose expected value already equaled the candidate value; the harness rejected both reviews and preserved rev4. Qwen had no Ollama tool-call parsing warnings in this run. Its lower elapsed time is one failed sample and does not establish a latency or quality advantage.

Under the later four-field review contract, the Gemma diagnostic stopped earlier on invalid outline ownership while Qwen reached the exact revision-12 resource prefix and then failed the finalize packet transport contract. The Qwen trace was 3.7 times slower by harness elapsed time and used substantially more model calls, but the two rows are single stochastic runs and cannot establish a model ranking. Neither final-contract run completed or produced current validation and simulation stamps.

The later Ornith diagnostic stopped at the same turn number as the four-field Gemma row but preserved only revision 2 because its surface review missed the requested button. Its shorter elapsed time reflects less completed work. All three model rows remain single stochastic diagnostics; Qwen reached the deepest exact prefix in these samples, while none completed StudyRoom. Only the Ornith row exercised the final reviewer-state isolation.

Each live-model row is one run. They are diagnostic traces, not reliability estimates, and must not be pooled with the historical samples. The historical one-shot StudyRoom result remains 0/3 completion, and the earlier adaptive five-turn experiment remains 0/3 completion. A single successful typed run, if obtained, demonstrates feasibility only. Default promotion still requires the full acceptance matrix: 3/3 isolated resources, 3/3 isolated finalize, 3/3 incremental turn five, at least 2/3 validate/simulate, and 9/10 product regressions.

## 7. Gemma Intent cohort and bounded recipe details

Measured 2026-07-14 through 2026-07-15 against `gemma4:12b-mlx` with a declared 16,384-token context. Promptfoo cache was disabled, concurrency was one, and the gateway URL and API key were supplied only through the environment and Keychain. These reports exercise the deterministic Intent IR → adjudication → managed-recipe compiler path rather than the historical mutation-tool planner. Raw reports remain in the ignored `results/` directory.

The V3 serving contract uses one bounded Core extraction call for default, clarification, and rejection paths. A recognized managed recipe that needs non-default copy, naming, or controls uses one additional detail call. The second call receives only its fixed system contract, the current human turn, and a harness-authored state binding. Its strict schema contains only the active detail facets. Every accepted scalar and nonempty naming affix must occur as an exact, case-sensitive, contiguous literal in the current human turn before the recipe can be compiled.

| Evaluation | Cases × repeats | Promptfoo pass | Mean harness ms | p95 harness ms | Model/tool calls | Compiled operations | Current validate/simulate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Default and boundary cohort | 10 × 1 | 10/10 | 10,677 | 17,605 | 1 or 2 / 1 or 2 | 22 for five ready paths; 0 for five non-ready paths | 5/5 ready paths |
| Grounded contrast cohort | 4 × 3 | 12/12 | 7,523 | 8,890 | 1/1 | 0 | not applicable |
| Full custom copy, naming, and controls | 1 × 3 | 3/3 | 19,209 | 19,811 | 2/2 | 22 | 3/3 |
| Copy-only reduced frontier | 1 × 1 | 1/1 | 21,610 | 21,610 | 2/2 | 22 | 1/1 |
| Post-refactor default and contrast regression | 14 × 1 | 14/14 | 10,827 | 17,737 | 1 or 2 / 1 or 2 | 22 for five ready paths; 0 for nine routed paths | 5/5 ready paths |

The default cohort includes five preview-ready managed-recipe paths and five intentional clarification or refusal paths. Therefore `completed` is 5/10 while the evaluator result is 10/10; the non-ready outcomes are expected behavior, not failures. The contrast cohort contains four distinct negative or clarification requests repeated three times. Its 12/12 result establishes deterministic boundary behavior for that sample, not general production reliability.

The full custom case requested all three detail facets and produced the exact requested launcher label, naming templates, Help/Join copy, and Close control. Every run compiled 22 deterministic operations, reached current validation and golden-trace simulation at revision 22, used exactly two model calls and two tool calls, and needed no repair. All three RuleSets were byte-identical and all three `compiled_plan_hash` values were identical.

The three custom runs did not have one stable semantic audit identity. `semantic_ir_digest`, `adjudication_digest`, `semantic_intent_hash`, and `input_intent_hash` each had two distinct values. The active custom fields are exact-grounded and the executable output did not vary. Code-path analysis locates the remaining freedom in the model-authored `objective`: it is included in Core, adjudication, and semantic/input digests but is not consumed by the managed-recipe compiler. Existing default cases mask this issue by explicitly asking the model to copy one fixed objective. The correct next change is a versioned semantic-identity separation in which raw request evidence, authoritative typed semantics, non-authoritative display annotation, and executable plan identity are distinct. Until that V4 change and a repeated stability gate are complete, this cohort proves stable RuleSet compilation for the measured request but does not prove stable semantic receipt identity.

The copy-only case exercised the smallest non-default frontier. It changed only the launcher label to `Begin deep work`, preserved every naming and Help/Join default, omitted the Close button and Close rule, and still reached current validation and simulation. This one run proves the reduced route is wired end to end; it is not a reliability estimate.

Development checkpoints preceding the passing custom sample are retained as evidence rather than omitted. The first request failed with `MISSING_REQUIRED_FIELD`; routed and stamped variants failed with `EMPTY_REQUIRED_RECIPE_DETAIL`; the isolated nested-shape variant failed with `INVALID_FIELD_TYPE`; and the first flat-shape variant still returned empty arguments. Explicit field-to-human-literal mapping produced the first pass. Exact literal grounding, schema/parser parity, and the copy-only assertion were added afterward. These checkpoints are different revisions and cannot be combined into a pass rate.

After the detail implementation was split into facade, schema, parse, and validation modules, cases 0–13 were rerun once from clean commit `c1e9a37266df0ae460748c5220402c48be7f5755`. The report `results/v3-precustom-final-regression-1run.json` passed 14/14 with no provider errors. All five preview-ready paths compiled 22 operations and had current validation and simulation; all nine routed discussion, fallback, capability-gap, and rejection paths kept Draft revision zero with no compiled operation. Source and build commits matched, `source_dirty` was false, and every response served the exact pinned model. This verifies behavior preservation for that one post-refactor sample; it is not the required repeated acceptance cohort.

The measured custom path is fast enough for an internal preview interaction on this machine, but this section does not certify commercial readiness. It has no concurrent-load measurement, no long-session edit/recompile evidence, no stable semantic receipt identity, no production API or authentication benchmark, and no whole-plan Discord side-effect preflight. The historical typed-planner path remains useful for unsupported recipes, but its latency and variance make it a fallback rather than the first commercial path.

## 8. Intent V4 targeted failure rerun

Measured 2026-07-16 from clean commit `0688d640f9072f4ce95518eda0b7e89e03df45a0` against `gemma4:12b-mlx` with the declared 16,384-token context, disabled cache, and concurrency one. The evaluator selected only the six assertion-failing cases from `results/gemma4-intent-v4-smoke-d7884418f8c1.json`. The original clean-source smoke passed 20/26; this targeted rerun passed 6/6 with no provider errors, automatic HTTP retries, repair attempts, or repeated errors.

| Case | Result | Harness ms | Model/tool calls | Turns | Compiled operations |
| --- | --- | ---: | ---: | ---: | ---: |
| English paraphrase | pass | 19,017 | 1/1 | 1 | 22 |
| Control mutation | pass | 10,356 | 2/2 | 1 | 22 |
| Missing hub | pass | 9,777 | 2/2 | 2 | 22 |
| Missing hub after restart | pass | 7,082 | 2/2 | 2 | 22 |
| Korean defaults | pass | 6,943 | 1/1 | 1 | 22 |
| Copy-only mutation | pass | 15,053 | 2/2 | 1 | 22 |

Every result selected the managed recipe, produced a current validation and simulation stamp, preserved exact request and semantic identity coverage, and reported the clean committed source and exact served model. This is a one-sample regression check of the previously failing subset. It is not a replacement for a clean full 26-case run or the required repeated acceptance matrix.

## 9. Luna-medium native worker cutover canaries

Measured 2026-07-17 from clean commit `c03f0642dfe142f8a30e28e606fdcfac1d022e8a`, which includes the native-worker byte-accounting fix. Source and build commits matched and both dirty flags were false in every report. The active provider was `codex_chatgpt`, with exact model `gpt-5.6-luna`, `medium` reasoning effort, and ChatGPT authentication through the bearer-authenticated worker bound to `127.0.0.1:18181`. The raw worker was not exposed through Cloudflare. Promptfoo cache was disabled and each row is one run.

| Case | Result | Promptfoo wall ms | Harness ms | Model/tool calls | Prompt/completion tokens | Outcome |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| English StudyRoom | 1/1 | 17,508 | 5,071 | 1/1 | 7,674/105 | ready at revision 22 |
| Korean StudyRoom | 1/1 | 8,617 | 6,195 | 1/1 | 7,698/120 | ready at revision 22 |
| Stateful game capability gap | 1/1 | 30,815 | 17,350 | 1/1 | 7,679/469 | deterministic `capability_gap`, revision 0 |
| Direct live Discord mutation | 1/1 | 10,839 | 8,401 | 1/1 | 7,653/277 | deterministic `reject`, revision 0 |

Both StudyRoom requests compiled 22 deterministic operations and reached current validation and golden-trace simulation. The unsupported stateful-game request preserved its hard requirements and produced a mutation-free capability gap. The direct-live-mutation request was rejected without Draft mutation. The four representative canaries therefore passed 4/4 and exercised ready, capability-gap, and safety-rejection paths through the native worker.

These are single clean-source canaries, not a reliability or commercial-readiness result. They do not replace the repeated default, detail, mutation, equivalence, concurrency, saturation, and recovery cohorts. The retired `local.cloudflared.starring`, `local.llm-api`, and `local.ollama.server` services were disabled and unloaded after the canaries passed; the `gemma4:12b-mlx` model file was retained only for rollback and was not part of these results.
