# Luna worker SLO measurements

Date: 2026-07-17

Status: diagnostic baseline, not commercial certification

## Bound environment

- Git commit: `5b83db67a91366b953676beb22b5fc949650d227`
- Worker source SHA-256: `99b5fd9b0859c2e04f74b5b48b1006754f5f48952ea6e9be0c4c488f527d2c7d`
- Provider/model: `codex_chatgpt` / `gpt-5.6-luna`
- Reasoning/authentication: `medium` / `chatgpt`
- Codex CLI: `codex-cli 0.144.2`
- Node/platform: `v24.18.0` / `darwin arm64`
- Worker profile: two active requests, eight queued requests, 55,000 ms request deadline
- Listener: loopback `127.0.0.1:18181`
- Automatic retries: zero

The installed LaunchAgent, worker health boundary, worker source digest, clean
Git source at both ends of each run, and hash-covered evidence manifest agreed.
The plist file and listener endpoint were operational prechecks, not fields
cryptographically embedded in the evidence manifest.

## Live canary

Run `slo-2026-07-17t14-50-37-281z` executed the exact 15-call plan: one warmup,
four serial probes, four two-request waves, one correlated cancellation, and one
post-cancellation recovery probe.

| Measure | Result |
| --- | ---: |
| Acceptance verdict | pass |
| Observed/planned live calls | 15/15 |
| Correct completed invocations | 14 |
| Expected cancellations | 1 |
| Unexpected invocations | 0 |
| Total run duration | 59,215 ms |
| Non-warmup successful latency P50/P95/max, 13 rows | 4,324 / 8,843 / 8,843 ms |
| Serial latency mean/P95/max | 4,828.25 / 6,217 / 6,217 ms |
| Two-request-wave latency mean/P95/max | 4,635 / 6,225 / 6,225 ms |
| Serial throughput | 0.206473 invocations/s |
| Two-request-wave throughput | 0.392696 invocations/s |
| Parallel-to-serial throughput ratio | 1.901924 |
| Cancellation recovery | 31 ms |
| Maximum active/queued | 2 / 0 |
| Observed input/cached/output/reasoning tokens | 88,952 / 0 / 826 / 182 |
| Mean input/output tokens per 14 usage-known correct completions | 6,353.71 / 59 |
| Worker RSS maximum | 70,402,048 bytes |
| Worker CPU maximum | 3.0% |
| Resource samples/errors | 230 / 0 |

The acceptance artifact has no failed gate. It grants only
`diagnostic_complete` and `eligible_for_step_load`. It explicitly denies a
commercial SLO, production capacity, and annual availability claim. Usage for
the cancelled request is unobserved and excluded from the reported token
counters, so 88,952/826 is not a complete provider-consumption total. The
acceptance non-claim is `cancelled_request_token_usage_unobserved`.

## Step-load diagnostic

Run `slo-2026-07-17t14-52-14-875z` verified the passing canary and records its
run ID as the prerequisite. The current step artifact does not contain a
cryptographic hash chain to the canary manifest. It executed one wave at
requested concurrency 1, 2, 3, and 4. The worker active limit remained two, so
the higher tiers measured bounded local queueing rather than four simultaneous
Codex executions.

| Requested concurrency | Calls | Wave duration | Throughput | Latency range | Maximum queue wait | Maximum runner duration |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 5,052 ms | 0.197941 req/s | 5,047 ms | 0 ms | 5,043 ms |
| 2 | 2 | 5,053 ms | 0.395804 req/s | 4,901–5,027 ms | 0 ms | 5,024 ms |
| 3 | 3 | 9,717 ms | 0.308737 req/s | 4,952–9,712 ms | 4,948 ms | 5,641 ms |
| 4 | 4 | 18,075 ms | 0.221300 req/s | 4,423–18,071 ms | 4,739 ms | 13,329 ms |

| Aggregate measure | Result |
| --- | ---: |
| Acceptance verdict | pass |
| Correct/observed/planned invocations | 10/10/10 |
| Unexpected invocations | 0 |
| Total run duration | 37,926 ms |
| Latency P50/P95/max | 5,027 / 18,071 / 18,071 ms |
| Maximum active/queued | 2 / 2 |
| Input/cached/output/reasoning tokens | 63,538 / 0 / 617 / 157 |
| Worker queue-wait P50/P95/max | 0 / 4,948 / 4,948 ms |
| Worker RSS maximum | 70,778,880 bytes |
| Worker CPU maximum | 2.7% |
| Resource samples/errors | 147 / 0 |

Per-tier throughput is correct invocations divided by hash-covered wave
duration in `raw.json`. The evaluator revision 1 summary retains all raw waves
and exact worker metrics but exposes named serial/parallel aggregates only;
generic per-phase summary output should be introduced with an explicitly
versioned evaluator schema rather than changing old evidence interpretation.

## Operating decision

The routine active limit remains two. It produced 1.90 times the serial canary
throughput and the highest step-load throughput. Requested concurrency three
and four completed correctly but added queue wait and increased the tail, with
no throughput gain. The eight-entry queue remains a bounded operational buffer;
this two-entry maximum occupancy does not certify all eight queue slots under
commercial load.

After both runs the same worker instance was idle with accepted/settled counters
at 25/25. Metrics health reported 25 attempted and 25 written records, zero
pending records, zero write failures, and no last error.

## Evidence

- Canary directory: `~/Library/Application Support/Starring/slo-evidence/slo-2026-07-17t14-50-37-281z`
- Canary manifest SHA-256: `50c39a1a2d56b916353cffbf9ca1746cc07f61cbd449d6ed8fd0c15b02ea6ba4`
- Step-load directory: `~/Library/Application Support/Starring/slo-evidence/slo-2026-07-17t14-52-14-875z`
- Step-load manifest SHA-256: `c66bccee0e464baf0aefcd20337f25ae9ac8371fc25b89141df126c72a84ec63`

Each evidence directory is mode 0700 and each artifact is mode 0600. The raw,
summary, acceptance, and manifest files stay local and are not committed.

## Explicit limits and next gates

This cohort measures the fixed transport frontier, scheduler, Codex identity,
usage, cancellation, metrics, and local queue behavior. It does not yet measure
the V15 Starring product adapter, real multi-turn authoring quality under load,
restart recovery, a six-hour soak, a 24-hour availability window, tenant
fairness, or authenticated public admission.

Resource sampling covers the Node worker PID, not the transient Codex child
process tree or whole-host contention. Those measurements must be added before
resource-capacity or memory-headroom claims. The observed cached-input token
count was zero in both live cohorts, so no prompt-cache saving is assumed.

Commercial promotion remains blocked until first-party product and failure
connectors, process-tree resource accounting, restart recovery, sustained soak,
and the declared commercial sample floors are implemented and pass in one
versioned candidate plan.
