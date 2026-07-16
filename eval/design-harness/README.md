# Design Harness Promptfoo benchmarks

This benchmark executes the real `design-harness-cli --eval-json` loop against the configured model edge. The legacy cohort uses an OpenAI-compatible gateway, while the active Intent cohort uses the authenticated Codex worker. It does not recreate the agent loop in JavaScript and it never reads or writes Discord, deployment, or production database state.

The two cohorts are intentionally separate.

- `promptfooconfig.yaml` and `cases.yaml` retain the historical adaptive and typed-plan benchmark, including its old oracle experiments.
- `promptfooconfig.intent.yaml` and `intent-cases.yaml` are the private-room Intent V4 checkpoint cohort. They are fixed to `gpt-5.6-luna` with `medium` reasoning effort, a declared 16384-token benchmark context policy, concurrency one, no cache, and evaluation input schema 3.

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

The typed-plan version 2 report contains `turns`, cumulative observability, per-turn observability deltas, revision continuity, and elapsed time. Intent input remains schema 3 while its report is schema 5 and its persisted session snapshot is schema 8. Typed-plan observability separates submissions, accepted requirements, compiled tool calls, execution failures, rollbacks, commits, and conflicts. `actual_gates` reports stamps that the model really earned during the session. `postcheck` reports the evaluator's non-mutating validation and simulation checks. These are intentionally separate so a successful evaluator postcheck cannot be mistaken for a gate the model called.

The legacy cases cover a complete one-shot request, an ambiguous request that should ask one blocking question, multi-turn elaboration, additive and replacement revisions that must preserve earlier work, the complex StudyRoom one-shot stress test, and typed-plan StudyRoom one-shot, five-turn, resource, and finalize paths. Production typed-plan cases delegate every call to the configured model. Oracle cases inject only their declared controls and separately measure the remaining delegated model calls.

The 26 Intent Recipe cases cover English and Korean one-shot builds, a pure single-turn English paraphrase, independent hub, locale, close, copy, naming, and control mutations, a two-turn missing-hub decision, a SQLite save and connection close/reopen while that decision is pending, discussion followed by build, typed-planner routing, creator-only and stateful-runtime capability gaps, gate-bypass distinctions, safe redaction, an exact unknown external capability, full custom recipe details, copy-only details, live-mutation and secret-disclosure rejection, and five request-grounding regressions. The grounding cases pin compound-target holds, Korean compound discussion, multi-sentence copied-command isolation, preview disambiguation, and discussion restart continuity. The prompts are natural user requests. They do not contain tool calls, raw RuleSet JSON, compiled requirements, fixtures, evaluator controls, or model-authored objective instructions.

## Local checks

Run from the repository root:

```sh
PATH=/opt/homebrew/opt/rustup/bin:$PATH cargo build -p design-harness-cli
npm --prefix eval/design-harness ci
npm --prefix eval/design-harness run check
```

## Luna medium Intent checkpoint

The repeated checkpoint is orchestrated by `matrix.js`. It must run from a clean committed source tree against the exact worker endpoint `http://127.0.0.1:18181`. The dedicated certification worker must report Luna medium, ChatGPT authentication, `codex-cli 0.144.2`, concurrency one, queue capacity zero, a 55000 ms request timeout, an idle scheduler, one stable instance ID, and a source digest equal to the local worker modules. The matrix refuses a dirty or changed source tree, a different worker instance or source, a different toolchain identity, and an output path outside the ignored `results/` directory.

The exact schedule contains 26 cases, 232 samples, 272 scripted turns, and 298 expected Luna calls. It is split into 27 interruption-bounded phases: one 26-case smoke phase followed by one supplemental phase for each case. Twenty-two cases reach ten samples; the independent hub, naming, control, and close mutation cases reach three. Every phase uses Promptfoo concurrency one with cache, sharing, Promptfoo persistence, and automatic HTTP retries disabled. The runner executes the `design-harness` Rust tests and the evaluation JavaScript tests before model work, assigns one run ID and contiguous run orders, writes each completed phase atomically, and validates the worker, source, and phase evidence at every boundary.

Before every sample the provider invokes `cargo build --locked -p design-harness-cli`, forbids an alternate harness binary, hashes the executable, and requires the child to match that digest and its embedded build commit and dirty state. The report records the runner source, local unsigned build identity, opaque worker identity, exact binding fingerprint, pinned harness budgets, run order, timestamps, requested model, served-model provenance observed in successful HTTP responses, declared context policy, recipe component revisions, registry digest, and per-attempt outcome, HTTP status, request, message, tool, duplicated-schema, token, finish-reason, and client-observed request wall-time metrics. Failed attempts record a null finish reason. Discussion quality requires the final successful metric to end with `stop` or `tool_calls`, accepts `stop` when a JSON-text response is promoted locally, and rejects completion-limit endings. `request_duration_ms` is client-observed wall time. The worker-populated `gateway_model_duration_ms` includes worker scheduling, identity checks, and Codex execution and must not be described as model-only latency. `burst_elapsed_ms` measures all model-facing work, while `elapsed_ms` includes SQLite save and any close/reopen/restore. The worker does not expose its active context window, so `gateway_context_observed_tokens` is deliberately null and 16384 is not described as a worker observation.

The planned wall time is 45–60 minutes based on the pre-matrix Luna canaries, not a completed acceptance measurement. The report propagates total prompt and completion tokens, but it does not yet propagate the worker's cached-input and reasoning-output subdivisions. Matrix artifacts therefore cannot claim uncached-input cost or visible-versus-reasoning output totals.

Commit all intended code and documentation before running. Restart the worker from that commit in the dedicated one-active, zero-queue profile, verify it is idle, and do not send any other completion request to port 18181 during the run. A dry run validates the clean source, exact plan, and local tooling without contacting the worker or creating the output directory.

```sh
cd /Users/jungbogeon/starring/eval/design-harness
OUTPUT="results/luna-v4-acceptance-$(date -u +%Y%m%dT%H%M%SZ)"
npm run matrix:intent -- --output "$OUTPUT" --dry-run
export STARRING_CODEX_WORKER_URL=http://127.0.0.1:18181
export STARRING_CODEX_WORKER_TOKEN="$(security find-generic-password -s com.starring.llm-api-key -a llm-api -w)"
npm run matrix:intent -- --output "$OUTPUT"
unset STARRING_CODEX_WORKER_TOKEN STARRING_CODEX_WORKER_URL
```

Use the same output directory to continue an interrupted run while the committed source, tooling, worker source, and worker instance are unchanged:

```sh
cd /Users/jungbogeon/starring/eval/design-harness
OUTPUT="results/luna-v4-acceptance-20260717T000000Z"
export STARRING_CODEX_WORKER_URL=http://127.0.0.1:18181
export STARRING_CODEX_WORKER_TOKEN="$(security find-generic-password -s com.starring.llm-api-key -a llm-api -w)"
npm run matrix:intent -- --output "$OUTPUT" --resume
unset STARRING_CODEX_WORKER_TOKEN STARRING_CODEX_WORKER_URL
```

Completed phases are checksum-verified and reused. An unfinished or failed phase is rerun with its attempt count preserved. Such a retry remains useful for diagnostics but cannot produce a retry-free certification; a fresh certifying attempt requires a new output directory and one execution of every gate and phase. A complete Promptfoo assertion-failure phase is preserved without selective rerun, and its nonzero result makes the final certification fail.

`state.json` is the continuation journal, `phases/` contains the raw phase reports, and `combined.json`, `summary.json`, and `failures.json` are derived evidence. The certification authority is the pair `manifest.json` and `acceptance.json`: the manifest binds the source, worker, tooling, plan, phases, observed totals, and SHA-256 identities of the derived artifacts; the acceptance artifact carries the model checks and matrix-level certification failures. A pass may be claimed only when both artifacts report `status: "passed"`, `acceptance.json` reports `pass: true`, its certification-failure list is empty, and the manifest-bound artifact digests match. Console output, `state.json`, or `summary.json` alone is not a verdict.

The worker's monotonic accepted and settled completion counters isolate each phase from unrelated valid requests. The matrix requires an idle worker before and after a phase, requires accepted and settled deltas to agree with the model calls in that phase, and preserves those boundaries in the run evidence. A counter mismatch, a worker restart, or a valid request from another client invalidates the cohort instead of being averaged into its latency or token measurements.

The Intent provider always requests exactly `gpt-5.6-luna` with `medium` reasoning effort; changing `STARRING_LLM_MODEL` or `STARRING_CODEX_REASONING_EFFORT` cannot create a mixed Intent cohort. It requires `STARRING_CODEX_WORKER_URL` and `STARRING_CODEX_WORKER_TOKEN`, passes both to the CLI without translating them into the legacy gateway variables, and derives the opaque endpoint identity from the worker URL. The CLI independently rejects a different served model. Acceptance requires non-overlapping run timestamps, exactly one attempt for every logical model call, and no automatic retry.

`acceptance.js` requires the exact 26-case manifest, at least ten samples for every non-mutation case and at least three for each independent mutation case, every Promptfoo component assertion to pass, 100% selection for the ten-run recipe cases, exact compiled-operation and gate evidence, zero unnecessary questions for complete requests, 100% required-decision resolution, exact mutation-free fallback routing, one model and one frontier call per default turn, two calls for selected detail turns, zero automatic HTTP retries, oracle isolation, and per-case repeat stability. The provider response must contain its own serialized string output and metadata object, and both must parse to the same report; a top-level fallback output or an object-valued response output is rejected. Every preview must carry request-evidence, compiler-input, semantic-intent, compiled-plan, candidate-RuleSet, and candidate-Draft identities, and the evaluator independently recomputes both candidate hashes from the reported RuleSet and Draft stamps. The English one-shot, pure paraphrase, confirmed multi-turn, restored multi-turn, discussion-then-build, and equivalent normalizer preview forms must have identical semantic hash, compiled plan hash, and final RuleSet. One-shot request evidence must differ from clarification, while clarification and restart identities remain continuous. Every hub, locale, close, copy, naming, and control mutation plus the full-custom combination must be repeat-stable and pairwise distinct from the default and each other across request, route, compiler, semantic, plan, RuleSet, and Draft identities. All turn and final route decisions participate in visible projection collision checks. A pinned axis-specific class matrix requires route identity equality for Core-equivalent default, English discussion, custom-static, and clarification/restart projections while keeping distinct route classes pairwise separate. Request and adjudication use narrower evidence classes: only exact shared human sequences may be equal, and every other evidence provenance must remain distinct even when its route semantic is equal. Normalizer discussion cases must remain mutation-free and the normalizer restart case must preserve its transcript and durable stage before building. One semantic identity must map to one RuleSet; distinct semantic outcomes may legitimately compile to the same RuleSet. One-call preview latency remains below 8 seconds at P50 and 20 seconds at P95. Two-call preview latency, including independent naming and control mutations, remains at or below 22 seconds at P50 and 30 seconds at P95. Every turn remains within 60 seconds.

This is an Intent V4 identity and local-serving checkpoint, not a commercial-readiness certificate. It covers default, full custom-detail, copy-only, and any-member Close builds, but it does not cover live stale/conflict prompts, a separate-process restart, multi-writer load, throughput, compensation, or external Discord failure recovery. Those remain explicit follow-up work even if the checkpoint passes.

The summarizer never merges metadata silently. It reports model, declared context, whether gateway context was observed, opaque gateway identity, source and binary identities, binding fingerprint, run ID and order span, timestamp span, dirty-source rate, oracle-isolation rate, recipe-selection rate, deterministic operation counts, identity coverage, request and schema bytes, token coverage, HTTP-attempt outcomes, client-observed request duration, gateway-model-duration coverage, burst duration, total turn duration, and latency percentiles. `metadata_mixed: true` or more than one metadata boundary invalidates the checkpoint.

Historical Gemma and Qwen measurements remain in `measurements.md` as prior diagnostic evidence. They must not be changed or used to satisfy the Luna medium Intent checkpoint.
