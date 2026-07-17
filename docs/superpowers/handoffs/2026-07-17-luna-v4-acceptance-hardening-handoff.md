# Luna V4 acceptance hardening handoff

Date: 2026-07-17

Branch: `feat/luna-v4-failure-cluster-hardening`

Current certified evidence source: `7f138b308644f954cd38ceee78768f3d6b7bf551`

Current matrix run ID: `luna-v4-4a7d40aa-e117-406e-b085-af6c3d63d37f`

Current matrix directory: `eval/design-harness/results/luna-v4-acceptance-v15-20260717T112904Z`

## Outcome

The current Luna V4 functional acceptance matrix is green on a clean source commit. All 232 Promptfoo rows passed, all 298 planned model calls and 298 tool calls completed on their first HTTP attempt, every acceptance check passed, and the final manifest status is `passed`.

The normalizer-12 run below remains the first certified pass for the 26-case matrix. The current normalizer-15 run adds a fail-closed complete-discussion boundary after two interrupted diagnostic runs exposed incomplete model output and an evaluator false positive. It does not establish commercial load or availability readiness.

## Scope and invariants

The work preserved the existing safety boundary:

- The model proposes typed intent; deterministic code adjudicates and compiles it.
- No activation or deployment route was added.
- No live Discord mutation was performed by the evaluation.
- Unsupported persistent runtime behavior remains a mutation-free capability gap.
- Direct live mutation, gate bypass, and secret disclosure remain deterministic rejections.
- Provider credentials remained outside source and artifacts.
- The native worker remained bound to loopback and was not exposed through Cloudflare.
- Model selection remained exact `gpt-5.6-luna` with `medium` reasoning effort and ChatGPT authentication.

The result is scoped to the single-worker profile declared by the matrix. It must not be represented as proof of concurrency, saturation, soak, live-side-effect, failover, or high-availability behavior.

## Change sequence

The branch hardened one failure family at a time and kept each functional concern in a separate commit. The final sequence relevant to this checkpoint is:

| Commit | Purpose |
| --- | --- |
| `7622f89` | Canonicalize boundary-only capability evidence |
| `76b969a` | Close static redaction semantics |
| `cf1f627` | Canonicalize runtime-only automation kind |
| `885c92b` | Rotate the V4 normalization identity |
| `e3c1f8a` | Refresh V4 normalization documentation pins |
| `898807f` | Refresh CLI V4 identity pins |
| `65b6557` | Align evaluator route classes with the V4 semantic contract |
| `a964ef6` | Distinguish reasoning-token usage from actual response truncation |
| `60bcf39` | Canonicalize runtime-only objective framing and rotate normalizer identity to 12 |
| `46a672b` | Record the historical normalizer-12 certificate |
| `f475395` | Align the operational serving terminology |
| `b1ce7b6` | Preserve grounded redaction requirements across sentence boundaries |
| `1689c22` | Carry preview-validation evidence across sentence boundaries |
| `dbeaa06` | Add evaluator discussion-response completeness assertions |
| `0ab2962` | Rotate the V4 normalization identity to 13 |
| `2b2bbd5` | Reject incomplete discussion responses at the Rust boundary |
| `0193ada` | Rotate the V4 normalization identity to 14 |
| `bfc6977` | Require explicit complete terminal punctuation after closing wrappers |
| `ad690b4` | Rotate the V4 normalization identity to 15 |
| `7f138b3` | Independently enforce complete terminals in the evaluator |

The earlier boundary and redaction changes are covered by the targeted 73-sample cohort. Commits `65b6557`, `a964ef6`, and `60bcf39` address findings from the first complete matrices. Commits `dbeaa06` through `7f138b3` address the later discussion-completeness diagnostics and keep the Rust and evaluator boundaries aligned.

## Evidence chronology

### Targeted failure cluster

Run `luna-v4-failure-cluster-20260717T053045Z` executed from clean commit `898807f2e2b232ee6e61085dc09786e134d29c93`.

| Evidence | Result |
| --- | ---: |
| Samples | 73/73 pass |
| Model/tool calls | 86/86 |
| Provider errors | 0 |
| Repairs | 0 |
| Automatic retries | 0 |
| Repeated errors | 0 |

All eight selected cases had one route identity and one adjudication identity within the case. This was a successful targeted gate, not a full-matrix certificate.

### First complete matrix and taxonomy defect

Directory `luna-v4-acceptance-20260717T055208Z` also used clean commit `898807f2e2b232ee6e61085dc09786e134d29c93`.

| Evidence | Result |
| --- | ---: |
| Promptfoo rows | 232/232 pass |
| Model/tool calls | 298/298 |
| Prompt/completion tokens | 2,143,644 / 47,978 |
| Provider errors / repairs / retries | 0 / 0 / 0 |
| One-call P50 / P95 | 5,814 / 9,154 ms |
| Two-call P50 / P95 | 11,383 / 14,731 ms |
| Maximum interactive turn | 20,994 ms |

Certification failed only `decision_identity_class_matrix`. The evaluator declared `intent_reject_live_mutation` and `intent_reject_skip_approval` as different route classes while both correctly projected to the same V4 closed aggregate route identity. V4 intentionally aggregates validation, preview, and approval bypass into one route-level safety boundary. The two requests still had different request-evidence hashes and different adjudication digests.

Changing production hashing to force artificial route separation would have changed the V4 semantic contract and required a new identity revision. Commit `65b6557` instead corrected the evaluator taxonomy: both cases share one route class while their request and adjudication classes remain separate. Unit coverage proves both the expected shared route identity and fail-closed behavior if either route diverges.

### Fresh matrix and two real failure signals

Directory `luna-v4-acceptance-20260717T065650Z` ran from clean commit `65b655715a4ce8f05eaa09d0e7b86c6ec1bd911d`.

| Evidence | Result |
| --- | ---: |
| Promptfoo rows | 230/232 pass |
| Model/tool calls | 298/298 |
| Provider errors / repairs / retries | 0 / 0 / 0 |
| Worker accepted/settled delta | 298/298 |
| Request counters | clean |

One discussion row was concise, complete, and terminated with an acceptable finish reason, but the evaluator rejected it because `completion_tokens` was 527. That count includes hidden reasoning usage and is not evidence of output truncation. Commit `a964ef6` removed the numeric completion-token proxy while retaining checks for truncation finish reasons, response length, sentence count, unfinished endings, unbalanced delimiters, headings, tables, and long lists.

The stateful-game row exposed a separate semantic issue. The model correctly extracted the required persistence, timers, economy, and event-time decision constraints but also repeated the clause head `Build a persistent Discord game` as an unmapped capability. That redundant wrapper changed route and adjudication identity for one repetition. Commit `60bcf39` added narrow syntax-aware ownership for the exact asserted objective head when runtime-only hard requirements are already represented. It does not discard arbitrary unmatched text: the match requires the bounded objective phrase, the supported automation-kind context, restart persistence, and independently detected runtime business spans. The commit rotated the normalizer revision from 11 to 12 and refreshed the registry digest.

### Targeted normalizer V12 proof

Before spending another complete matrix, clean source `60bcf398bbf10eb7c4fbe7c40f03ddd966455fba` ran the two affected live cohorts.

| Cohort | Result | Calls |
| --- | ---: | ---: |
| `luna-v4-normalizer-v12-stateful-30-20260717T082144Z-live.json` | 30/30 pass | 30 model, 30 tool |
| `luna-v4-normalizer-v12-discussion-10-20260717T082144Z-live.json` | 10/10 pass | 10 model, 10 tool |

Both remained first-attempt, provider-clean, and repair-free. These targeted rows validated the two fixes but were not used as a substitute for the final full matrix.

## Historical V12 certified matrix

The authoritative final result is `eval/design-harness/results/luna-v4-acceptance-20260717T083524Z`.

| Field | Value |
| --- | --- |
| Run ID | `luna-v4-07dbba25-307e-41b7-8458-43c3f7d83e1b` |
| Evidence source | `60bcf398bbf10eb7c4fbe7c40f03ddd966455fba` |
| Manifest status | `passed` |
| Samples | 232 |
| Valid reports | 232 |
| Promptfoo passes | 232 |
| Model calls | 298 |
| Tool calls | 298 |
| Provider errors | 0 |
| Repair attempts | 0 |
| Automatic retries | 0 |
| Prompt tokens | 2,143,640 |
| Completion tokens | 49,668 |
| Worker accepted/settled delta | 298/298 |
| Model-call plan | exact 298/298 |
| Deterministic gates | passed, retry-free |
| Promptfoo exit | clean |

### Latency

| Slice | P50 | P95 |
| --- | ---: | ---: |
| One-call preview turn | 5,297 ms | 9,972 ms |
| Two-call preview turn | 10,193 ms | 14,813 ms |

The acceptance evaluator's maximum interactive-turn input was 21,454 ms, below the matrix hard boundary of 60,000 ms. The separate maximum harness-reported elapsed time was 26,829 ms and the maximum individual HTTP request duration was 21,388 ms. These values describe serial single-worker requests only. They do not predict queueing delay under concurrent demand.

### Semantic identity

| Axis | Declared classes | Collisions | Result |
| --- | ---: | ---: | --- |
| Request evidence | 25 | 0 | stable |
| Route semantics | 17 | 0 | stable |
| Adjudication | 25 | 0 | stable |

All within-case repeat identities were stable. The lower route-class count is intentional because multiple differently worded requests may share one authoritative semantic route while request evidence and adjudication remain separately auditable.

The final catalog identity is:

- Extractor revision: 16
- Normalizer revision: 12
- Compiler revision: 1
- Simulator revision: 1
- Recipe: `starring.private_study_room` version 1
- Registry digest: `5ab0dac8c5d445f01fad4bffaa91bf2eb8cfaa2b15c70ce6aa888b06be4253b7`

## Interrupted V13 and V14 diagnostics

V13 at `eval/design-harness/results/luna-v4-acceptance-v13-20260717T100554Z` used source `0ab2962d6dade69a8fbcbdaa17a38d1cb79fd968`. It saved 88 rows across 11 complete phases: 87 passed and one same-target discussion failed because it ended with `whereas explicit approvals,`. The saved rows contain 118 model/tool calls and 845,161/16,191 prompt/completion tokens; checkpoint counters moved exactly 298/298 to 416/416. The run was stopped with phase 12 lacking an artifact and no finalization outputs. Its `running` state is non-certifying.

V14 at `eval/design-harness/results/luna-v4-acceptance-v14-20260717T105242Z` used source `0193ada3b610e3f73a6794f3eb11cc9810843e06`. It saved 35 rows across two complete phases, with 43 model/tool calls, 311,272/6,390 prompt/completion tokens, and counters moving exactly 435/435 to 478/478. Although all saved rows were marked passing, `intent_normalizer_discussion_restart_then_build` contained a response ending in the bare words `which can feel helpful for focused study`. This exposed a real evaluator false positive. The third phase has no artifact and the run never finalized, so it is diagnostic only.

Neither interrupted run is resumed, pooled, or used in the V15 denominator. Calls observed outside their last committed phase checkpoints have no result artifacts and are not included in their saved metrics.

## Current V15 certified matrix

The authoritative current result is `eval/design-harness/results/luna-v4-acceptance-v15-20260717T112904Z`.

| Field | Value |
| --- | --- |
| Run ID | `luna-v4-4a7d40aa-e117-406e-b085-af6c3d63d37f` |
| Evidence source | `7f138b308644f954cd38ceee78768f3d6b7bf551` |
| Manifest status | `passed` |
| Samples / valid reports / Promptfoo passes | 232 / 232 / 232 |
| Model/tool calls | 298 / 298 |
| Provider errors / repairs / retries | 0 / 0 / 0 |
| Prompt/completion tokens | 2,153,127 / 48,445 |
| Worker accepted/settled delta | 298 / 298 |
| Deterministic gates / Promptfoo exit | retry-free pass / clean |

| Slice | P50 | P95 | Maximum |
| --- | ---: | ---: | ---: |
| One-call preview turn | 5,789 ms | 11,728 ms | — |
| Two-call preview turn | 10,479 ms | 13,202 ms | — |
| HTTP request | 6,543 ms | 11,799 ms | 22,799 ms |
| Interactive turn | — | — | 29,556 ms |
| Harness report | — | — | 29,561 ms |

The 29,556 ms maximum was one `custom_details` two-call turn at run order 36. It passed the 30-second two-call P95 gate and 60-second hard limit, but it remains an explicit latency-tail target for the commercial SLO program.

Request, route, and adjudication identities formed 25, 17, and 25 declared classes with zero collisions. Every report pins extractor 16, normalizer 15, compiler 1, simulator 1, and registry digest `fc66223bee4c1ec2e3dd2535a4a4ad1dae6a17f3b896b1a29a6998cde4d8535c`. The clean source's catalog regression separately pins descriptor digest `9b24010cd9327f2981ad841eac9afeaf404665dcc757171be2d35095648d1b0b`.

The current manifest binds:

| Artifact | SHA-256 |
| --- | --- |
| `combined.json` | `1fbb17bfdad8ce14ebd16d6c21e4584c9baaf38d7ee786d298b06f5b225bdefb` |
| `summary.json` | `616ac32668b86fa66ac3aadaa570479db5a162787b857f6308977e16238011af` |
| `acceptance.json` | `5c55446487c00052bfbc24817d4450881077e743f67d87cdac32bae3b908dd59` |
| `failures.json` | `4041b703721391b2c974efe3a62469d3d11083b1bc8f1db6eb32d91b9178575a` |

The independently computed SHA-256 of `manifest.json` is `3376e3943459023b525c6de077792ea677664c25e6dfcf17c8b912c08fe6788c`.

## Worker and toolchain boundary

The final dedicated worker reported:

| Field | Value |
| --- | --- |
| Provider | `codex_chatgpt` |
| Model | `gpt-5.6-luna` |
| Reasoning effort | `medium` |
| Authentication | `chatgpt` |
| Codex CLI | `codex-cli 0.144.2` |
| Concurrency limit | 1 |
| Queue capacity | 0 |
| Request timeout | 55,000 ms |
| Instance ID | `def63586-24fd-4993-9207-f12e7ca7c7ac` |
| Worker source digest | `afe1de6c201300c2734d862b854fa1faa104ea1e213f2bd83b7310f3f2e8da51` |

The manifest also pins Node `v26.5.0`, Promptfoo `0.121.18`, Cargo `1.97.0`, their executable or package hashes, the lockfile hash, the binary hash, the binding fingerprint, the evaluation-source hashes, and the complete phase schedule.

## Historical V12 artifact integrity

The final manifest binds the core artifacts as follows:

| Artifact | SHA-256 |
| --- | --- |
| `combined.json` | `4de58b36f240bde1f9edb2536f9284983f1a85ee54dc3263e942ddaf75d5fd21` |
| `summary.json` | `c330c9bfd7292f0075c929dad1a3b7ef9c95912d45b97dc83c99b06660addf9d` |
| `acceptance.json` | `59d42d18d8156700cbe9dd0197662ea652028913a16b61e7e6a96728d0db9e77` |
| `failures.json` | `4041b703721391b2c974efe3a62469d3d11083b1bc8f1db6eb32d91b9178575a` |

The finalizer pins:

| Evaluator source | SHA-256 |
| --- | --- |
| Matrix runner | `d1659da5710485a5b0e0429f7754094a8d9afaf37c8335607ff6c5dbc99eab99` |
| Acceptance evaluator | `a19b409fa8006af57b06429dec90ec1e08c7f964018dab1f6279409d19c12578` |
| Summarizer | `578fa3b6c0cf77a8503dc767932ed994604f5625b338573b50d3ce276abbb9f1` |

The results directory is locally ignored evaluation evidence. Preserve it on the home server until the PR has been independently reviewed and the relevant evidence has been archived according to the project policy. Do not treat an unverified copy with the same directory name as equivalent; verify the manifest-bound hashes.

## Gates at the certified source

The matrix ran both declared deterministic gates exactly once before the live schedule:

- `cargo test -p design-harness`: passed
- Evaluation JavaScript test suite: passed

The runner recorded both gates as retry-free and bound their output hashes in the manifest. The live schedule then completed once without selective row reruns. The final certificate was produced inline from the same clean source and worker instance.

An independent current-source audit reran `cargo test -p design-harness` and observed 878 passing tests across its targets, then reran the evaluation `npm test` suite at 106/106. The CLI passed 82 tests plus its dependency guard; relevant clippy and formatting gates were green. The audit recomputed the phase and final artifact hashes, independently aggregated the 232 rows and 298 calls, and found no Keychain worker secret or Cloudflare, AWS, R2, bearer, or authorization credential pattern in the current artifacts.

GitHub Actions run 31 for source `7f138b3` passed both the complete workspace `checks` job and the PostgreSQL integration job. Pull request 7 remains the integration vehicle; its final documentation head must also be green before merge.

## Explicit non-claims

The result does not certify:

- Multiple simultaneous users or worker processes
- Queueing behavior or admission control under load
- Throughput at a target requests-per-second rate
- Long-duration soak stability or memory growth
- Worker restart while requests are active
- Codex authentication refresh and quota-exhaustion behavior
- Network partition, disk pressure, or process crash recovery
- Multi-instance coordination or duplicate suppression
- High availability, failover, backup, or disaster recovery
- Live Discord permission preflight or side-effect correctness
- Cloudflare exposure, public API authentication, abuse protection, or tenant isolation
- Commercial service-level objectives

The matrix demonstrates deterministic functional quality and serial latency for its pinned cases and environment. It is a necessary commercial-readiness input, not the complete commercial-readiness decision.

## Exact continuation order

1. Commit and push the current evidence documentation without changing the certified source identity.
2. Require the documentation-head GitHub Actions `checks` and `postgres` jobs to pass.
3. Confirm that no new actionable review finding was introduced.
4. Merge pull request 7 into `main`.
5. Synchronize the home-server checkout to merged `main` and verify the loopback Luna worker profile and health.
6. Begin the commercial SLO program on a new branch.

## Next goal after merge: commercial SLO program

The next work should convert the current serial baseline into an explicit operating envelope. Define targets before implementation or tuning:

- Interactive latency targets for one-call and two-call paths at P50, P95, and P99
- Maximum queue wait and end-to-end deadline
- Sustainable throughput per worker
- Allowed concurrency and queue capacity
- Error, timeout, cancellation, and retry budgets
- Token and Codex-usage budgets per accepted request
- Recovery-time and recovery-point objectives
- Availability and maintenance-window expectations

Then build a separate, reproducible test program in this order:

1. Concurrency correctness with small fixed cohorts and exact request accounting
2. Step-load saturation to find the safe single-worker envelope
3. Queue admission, cancellation, deadline, and overload behavior
4. Worker restart and authentication-drift recovery
5. Multi-hour soak with memory, process, disk, and latency telemetry
6. Multi-worker or high-availability design only if the measured service target requires it
7. Live Discord preflight and side-effect validation in a disposable environment
8. Public API and Cloudflare security review before any external exposure

Commercial promotion should require a new acceptance document and evidence set. The 232/232 Luna V4 certificate remains the functional baseline against which all serving changes must be regression-tested.
