# Luna commercial SLO program design

Date: 2026-07-17

Status: diagnostic implementation measured; commercial certification blocked

Branch: `feat/luna-commercial-slo-program`

## Objective

Define and measure the operating envelope of the loopback Luna-medium authoring
worker on the Mac mini without weakening the existing model, identity, safety,
or candidate-only boundaries. The program must distinguish functional quality,
worker capacity, queue behavior, recovery, and sustained operation instead of
combining them into one pass rate.

The immediate outcome is a reproducible diagnostic baseline. Commercial SLO
certification remains a later outcome and requires the sample floors, recovery
tests, and soak duration in this document. A short canary must never be
relabeled as an availability or production-capacity certificate.

The first live canary and step-load results are recorded in
[`eval/codex-worker-slo/measurements.md`](../../../eval/codex-worker-slo/measurements.md).

## Current baseline

The functional baseline is the clean Luna V15 cohort at source
`7f138b308644f954cd38ceee78768f3d6b7bf551`:

- 232/232 evaluation rows and 33/33 acceptance checks
- 298/298 model and tool calls
- zero provider errors, repairs, retries, or identity collisions
- one-call preview P50/P95 of 5,789/11,728 ms
- two-call preview P50/P95 of 10,479/13,202 ms
- one 29,556 ms two-call tail observation

That cohort ran serially with one active slot and no queue. It certifies bounded
functional quality, not concurrent service.

The tracked routine LaunchAgent declares two active requests, eight queued
requests, and a 55,000 ms wall deadline. At the start of this program the
installed LaunchAgent still carries the prior acceptance-only profile of one
active request and no queue. No commercial measurement may start until the
installed profile, worker health, and evidence manifest agree.

## Safety and authority

- The worker remains bound to `127.0.0.1`; port 18181 is never a Cloudflare
  origin.
- The bearer token remains in Keychain and environment memory. It is never
  serialized into plans, artifacts, metrics, commands, or logs.
- The exact provider, model, reasoning effort, ChatGPT authentication mode, and
  Codex CLI version remain pinned.
- Codex execution remains ephemeral, read-only, approval-free, web-disabled,
  and tool-disabled.
- The program does not activate a RuleSet, deploy a candidate, access the
  production database, or mutate Discord.
- Automatic HTTP or model retries are forbidden in measurement cohorts.
- A provider, authentication, identity, timeout, source, instance, counter, or
  artifact-integrity failure stops later live phases.
- Only one live worker may invoke the shared ChatGPT login during a cohort.

## SLI model

The program records separate service-level indicators:

| SLI | Definition |
| --- | --- |
| Correct completion | HTTP 200, exact Luna identity, exact requested frontier, schema-valid grounded arguments |
| End-to-end latency | Client monotonic time from request dispatch through complete response body |
| Queue wait | Worker monotonic time from validated admission until runner start |
| Runner duration | Worker monotonic time from runner start until runner settlement |
| Throughput | Correct completed requests divided by the measured wave interval |
| Error rate | Non-200, invalid response, timeout, identity drift, or wrong output divided by validated attempts |
| Saturation | Maximum active and queued requests observed during a wave |
| Cancellation recovery | Time from client cancellation until the active slot, child process, and counters settle |
| Restart recovery | Time from an intentional worker stop until exact readiness returns |
| Resource stability | Worker RSS, heap, CPU, event-loop, file, and process observations across a bounded soak |
| Usage | Input, cached input, output, and reasoning-output tokens per correct completion |

Latency percentiles are computed from successful requests only, while failures
remain in the error-rate denominator. Queue rejection and timeout rows are never
removed from a cohort. Warmup rows are declared in the plan and retained in the
raw artifact even when excluded from percentile gates.

## Measurement layers

### Layer A: worker transport probe

A small fixed frontier returns a unique request sequence and fixed status. It
measures scheduler, queue, Codex process, identity, structured output, token
usage, and transport behavior without spending tokens on a full design prompt.

### Layer B: Starring authoring workload

Representative one-call and two-call Intent Recipe requests reuse the V15
contracts. They measure actual product latency and preserve validate/simulate,
identity, Draft-isolation, and candidate-only assertions. Transport probe rows
cannot substitute for these quality rows.

### Layer C: deterministic failure and recovery

Fake-runner and temporary-process tests exercise queue overflow, cancellation,
deadline exhaustion, ignored aborts, shutdown, metrics failure, authentication
drift, and restart without Luna usage. Live failure injection is used only after
the deterministic gates pass.

## Evidence architecture

The implementation is split by responsibility:

```text
tools/codex-worker/
  scheduler.mjs                 FIFO capacity and request-counter state
  request-timeline.mjs          monotonic per-request timing
  worker.mjs                    authenticated loopback HTTP edge
  metrics-log.mjs               bounded secret-free operational JSONL

eval/codex-worker-slo/
  plans.mjs                     closed diagnostic and certification plans
  workloads.mjs                 fixed worker and Starring workloads
  load-runner.mjs               wave execution and health sampling
  resource-sampler.mjs          bounded host and worker observations
  summarize.mjs                 deterministic aggregation
  acceptance.mjs                diagnostic and certification gates
  artifact-store.mjs            atomic private evidence files
  metrics-reader.mjs            bounded private worker-metric ingestion
  program.mjs                   clean-source orchestration and evidence sealing
```

The worker behavior is first moved without semantic change. Timeline telemetry
is a separate commit. The evaluation program is a third commit. Tuning is not
combined with measurement infrastructure.

## Worker telemetry contract

Operational metric records retain the existing fields and add a versioned,
secret-free request timeline:

- worker instance and source digest
- concurrency, queue capacity, and deadline profile
- active and queued counts at admission
- queue wait, runner duration, and total duration
- terminal stage, status, outcome, and bounded error code
- token usage only for successful validated responses

Prompts, tool schemas, outputs, bearer tokens, Codex diagnostics, and temporary
paths remain forbidden. Metrics-write degradation must be observable before a
cohort can certify. Worker timings use a monotonic clock.

## Artifact contract

Every run writes a new private directory with mode 0700 and files with mode
0600:

- `raw.json`: plan, worker boundary, health samples, resource samples, and all
  request observations
- `summary.json`: deterministic counts, latency distributions, throughput,
  saturation, usage, and recovery values
- `acceptance.json`: every named gate and explicit non-claims
- `manifest.json`: source, dirty flag, plan digest, worker identity and profile,
  toolchain, start/end counters, and hashes of the other artifacts

The manifest does not hash itself. Runs reject a dirty source, worker restart,
source mismatch, profile mismatch, non-idle boundary, counter discontinuity,
automatic retry, missing metric, or unbalanced final counter. Interrupted runs
remain diagnostic and cannot be resumed into a different worker instance.

Before any live call, the evaluator reserves an empty private run directory and
holds its directory identity through evidence sealing. The evaluator binds the
clean Git snapshot to the digest of the exact worker source files, requires the
running worker to expose that digest, and repeats the clean source snapshot
before sealing. Toolchain metadata lives in hash-covered raw evidence and the
manifest is derived from it.

The final metrics-health correlation and metrics-file read share a separate
real-time deadline linked to the run deadline. A stalled metrics endpoint or
reader therefore fails the diagnostic within the declared metrics phase instead
of consuming the rest of the plan budget.

An execution or post-processing exception is sealed as interrupted,
explicitly incomplete evidence whenever the reserved directory remains usable.
When execution state cannot be recovered, zero-filled usage counters are paired
with `live_call_count_known=false` and cannot support a zero-usage claim;
acceptance records corresponding non-claims. If evidence sealing itself fails,
the command reports the reserved run identifier and directory so the operator
can locate the failed run without printing secrets.

## Profiles and budgets

### Development profile

Uses a fake runner only. It may run thousands of scheduler operations and
process failures but makes zero Luna calls.

### Live canary profile

The first live budget is at most 15 Luna calls:

1. readiness and one warmup probe
2. four serial representative probes
3. four two-request waves, eight requests total
4. one cancellation and one post-cancellation recovery request if every
   earlier phase passes

The exact maximum is `1 + 4 + 8 + 1 + 1 = 15` live calls.

The runner stops before the next wave on the first error. The input budget is
130,000 tokens and the output budget is 5,000 tokens. Exceeding either budget
halts the run rather than borrowing from a later phase.

### Step-load diagnostic profile

After the canary passes, fixed waves measure concurrency 1, 2, 3, and 4. The
profile starts with one wave per tier. Repetitions are added only when the
observed capacity and token usage justify them. It is a capacity diagnostic,
not a commercial certificate.

### Commercial candidate profile

Certification requires all of the following in one versioned plan:

- at least 30 successful observations for every promoted concurrency tier
- both one-call and two-call Starring workloads
- bounded overload, cancellation, deadline, and restart scenarios
- at least six hours of resource telemetry
- a declared usage budget and no quota or authentication ambiguity
- no source, worker, profile, counter, metric, or artifact discontinuity

A 24-hour run is required before an availability claim. The program does not
infer annual availability from a shorter local test.

Commercial certification is intentionally disabled until the product adapter,
failure scenarios, source provenance, and resource sampler are bound to trusted
first-party connectors. Caller-supplied objects may support diagnostics but can
never produce a commercial certificate.

## Initial canary gates

These are pre-SLO promotion gates, not the final commercial targets:

- every non-cancelled request is correct
- provider, authentication, identity, timeout, and metrics errors are zero
- serial maximum is at most 25 seconds
- two-request-wave P95 is at most 25 seconds
- overall maximum is at most 45 seconds
- concurrency two improves completed throughput by at least 1.5 times the
  serial baseline; otherwise the routine active limit returns to one
- cancellation releases the active slot and balances counters within five
  seconds
- active and queued requests finish at zero
- accepted and settled counters have the exact planned delta
- source, instance, profile, request, metric, and manifest counts agree

These thresholds are deliberately fail-closed. Passing them authorizes a larger
diagnostic cohort, not public traffic.

## Failure classification and admission

Authentication drift, quota/provider failure, timeout, queue rejection, client
disconnect, invalid structured output, and internal runner failure must remain
separate bounded codes. The live runner has no retry loop. Repeated provider or
authentication failure must open a cool-down boundary before any sustained
load program; otherwise the service can create a process storm while consuming
no useful capacity.

The worker FIFO is not a tenant-fair production queue. Per-tenant in-flight
limits, session cancellation propagation, and fair scheduling belong at the
future authenticated backend boundary. The worker queue is only a bounded local
transport buffer.

The live cancellation probe uses an authenticated correlation identifier and
waits for that exact request to report active admission before disconnecting.
Aggregate counter movement or a queued request never counts as proof of active
cancellation.

## Implementation and verification order

1. Commit this design without changing runtime behavior.
2. Move scheduler and counter logic into a pure module and prove behavior
   preservation with existing and new deterministic tests.
3. Add monotonic request timeline and versioned operational metrics without
   changing response bodies or scheduling behavior.
4. Implement the artifact-bound SLO runner and fake-server test suite.
5. Add live readiness, metrics-degradation, failure-classification, and
   fail-fast gates needed by the canary.
6. Commit all measurement code, require a clean source, then restore the
   installed routine profile from the tracked 2/8/55,000 ms plist.
7. Verify loopback listener, exact identity, source digest, readiness, idle
   counters, log permissions, and single-worker ownership.
8. Run the at-most-15-call canary and report every failure honestly.
9. Run one-wave 1/2/3/4 step-load diagnostics only if the canary passes.
10. Tune concurrency, queue, admission, or identity checks only in later
    functional commits, rerunning the affected deterministic and live gates.
11. Add restart and short soak diagnostics before proposing a commercial SLO.

## Promotion rule

The initial program is complete when it can reproduce a clean diagnostic
artifact, explain where time and tokens are spent, restore routine operation
after any interruption, and state a measured safe envelope without hiding
failures. Commercial promotion requires a separate acceptance record proving
the candidate profile. Public exposure, multi-tenant admission, and an SLA are
out of scope until that record exists.
