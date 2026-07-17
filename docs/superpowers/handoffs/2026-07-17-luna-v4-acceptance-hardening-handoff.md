# Luna V4 acceptance hardening handoff

Date: 2026-07-17

Branch: `feat/luna-v4-failure-cluster-hardening`

Certified evidence source: `60bcf398bbf10eb7c4fbe7c40f03ddd966455fba`

Final matrix run ID: `luna-v4-07dbba25-307e-41b7-8458-43c3f7d83e1b`

Final matrix directory: `eval/design-harness/results/luna-v4-acceptance-20260717T083524Z`

## Outcome

The fixed Luna V4 functional acceptance matrix is green on a clean source commit. All 232 Promptfoo rows passed, all 298 planned model calls and 298 tool calls completed on their first HTTP attempt, every acceptance check passed, and the final manifest status is `passed`.

This is the first certified pass for the current 26-case Luna V4 matrix. It closes the observed boundary, redaction, normalization, identity-class, discussion-quality, and stateful-runtime failure clusters represented by that matrix. It does not establish commercial load or availability readiness.

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

The earlier boundary and redaction changes are covered by the targeted 73-sample cohort. The last three commits address issues exposed only after the first complete 232-sample executions.

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

## Final certified matrix

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

## Artifact integrity

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

An independent pre-push audit reran `cargo test -p design-harness` and observed 876 passing tests across its targets, then reran the evaluation `npm test` suite at 106/106. It also recomputed the phase and final artifact hashes, independently aggregated the 232 rows and 298 calls, and found no Keychain worker secret or Cloudflare, AWS, R2, bearer, or authorization credential pattern in the final artifacts or tracked source.

The pull request and remote CI are not part of this local certificate. They remain required before merge.

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

Do not begin new feature or commercial-load work on this branch before completing the integration sequence.

1. Confirm the branch contains only the intended functional commits and this evidence documentation, with no credentials or generated result artifacts staged.
2. Run the local pre-push gates required by the repository and record any result that is newer than the certified source separately. Documentation-only commits must not be presented as the model evidence source.
3. Push `feat/luna-v4-failure-cluster-hardening`.
4. Open a pull request targeting `main`. Include the failure chronology, final 232/232 result, 298/298 call accounting, latency, token usage, non-claims, and artifact hashes.
5. Wait for all GitHub Actions checks, including PostgreSQL-backed checks, to pass. Do not merge on partial or pending CI.
6. Obtain independent review of the production normalization boundary, evaluator taxonomy, identity-revision rotation, and result interpretation. Address findings in functional commits and rerun the affected gates; rerun the live matrix if production semantics or acceptance logic changes.
7. Merge the reviewed pull request into `main` only after CI and review are green.
8. Synchronize the home-server checkout to the merged `main` and verify the deployed worker still reports the pinned Luna-medium profile and loopback boundary.
9. Create a new branch and specification for commercial SLO work. Keep load and operational changes out of this acceptance-hardening branch.

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
