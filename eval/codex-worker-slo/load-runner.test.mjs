import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { startWorker } from "../../tools/codex-worker/worker.mjs";
import { assessRun } from "./acceptance.mjs";
import { runPlan } from "./load-runner.mjs";
import { createFileMetricsReader } from "./metrics-reader.mjs";
import { EXPECTED_IDENTITY, getPlan } from "./plans.mjs";
import { summarizeRun } from "./summarize.mjs";

const SOURCE = {
  commit: "a".repeat(40),
  dirty: false,
  worker_source_sha256: "b".repeat(64),
};
const TOKEN = "test-slo-token-123456";

function usage(input = 10, output = 2) {
  return {
    input_tokens: input,
    cached_input_tokens: 0,
    output_tokens: output,
    reasoning_output_tokens: 0,
  };
}

function metric(requestId, outcome, statusCode, metricUsage) {
  return {
    metric_schema_version: 2,
    timestamp: "2026-07-17T00:00:00.000Z",
    request_id: requestId,
    ...EXPECTED_IDENTITY,
    frontier_name: "record_slo_probe",
    instance_id: "test-live-instance",
    worker_source_sha256: "b".repeat(64),
    concurrency_limit: 2,
    queue_capacity: 8,
    request_timeout_ms: 55_000,
    active_at_admission: 0,
    queued_at_admission: 0,
    queue_wait_ms: 0,
    runner_duration_ms: 1,
    runner_elapsed_at_terminal_ms: null,
    post_runner_ms: 0,
    total_duration_ms: 1,
    runner_started: true,
    runner_settled: true,
    runner_outcome: outcome === "succeeded" ? "resolved" : "rejected",
    result_validation_started: outcome === "succeeded",
    terminal_stage: outcome === "succeeded" ? "completed" : "runner",
    outcome,
    status_code: statusCode,
    duration_ms: 1,
    usage: metricUsage,
    error_code: outcome === "failed" ? "client_disconnected" : null,
  };
}

function requestId(sequence) {
  return `00000000-0000-4000-8000-${String(sequence).padStart(12, "0")}`;
}

function liveFixture(configuration = {}) {
  let accepted = 0;
  let settled = 0;
  let completions = 0;
  let pendingCancellation = null;
  let cancellationAdmissionReads = 0;
  let cancellationAbortedBeforeAdmission = false;
  let hungRequestAborted = false;
  let unrelatedActive = false;
  const metrics = [];
  const health = () => ({
    schema_version: 1,
    status: "ok",
    ...EXPECTED_IDENTITY,
    instance_id: "test-live-instance",
    worker_source_sha256: "b".repeat(64),
    concurrency_limit: 2,
    queue_capacity: 8,
    request_timeout_ms: 55_000,
    active_requests: accepted - settled,
    queued_requests: 0,
    accepted_requests_total: accepted,
    settled_requests_total: settled,
  });
  const fetchFn = async (url, options = {}) => {
    const target = String(url);
    if (target.endsWith("/metrics-health")) {
      assert.equal(options.headers.authorization, `Bearer ${TOKEN}`);
      return new Response(JSON.stringify({
        schema_version: 1,
        instance_id: "test-live-instance",
        worker_source_sha256: "b".repeat(64),
        status: configuration.metricsStatus ?? "ok",
        writable_verified: configuration.metricsWritable ?? true,
        records_attempted: metrics.length + (configuration.metricsFailures ?? 0),
        records_written: metrics.length,
        pending_records: 0,
        write_failures_total: configuration.metricsFailures ?? 0,
        last_error_code: configuration.metricsFailures ? "metrics_write_failed" : null,
      }), { status: 200 });
    }
    if (target.endsWith("/health")) {
      assert.equal(options.headers.authorization, `Bearer ${TOKEN}`);
      return new Response(JSON.stringify(health()), { status: 200 });
    }
    if (target.includes("/request-admission?")) {
      assert.equal(options.headers.authorization, `Bearer ${TOKEN}`);
      const observationId = new URL(target).searchParams.get("observation_id");
      cancellationAdmissionReads += 1;
      if (configuration.unrelatedAdmissionAfterRead === cancellationAdmissionReads) {
        accepted += 1;
        unrelatedActive = true;
      }
      if (pendingCancellation
        && pendingCancellation.observationId === observationId
        && !pendingCancellation.admitted
        && !configuration.cancellationNeverAdmitted
        && Number.isSafeInteger(configuration.cancellationAdmissionAfterReads)
        && cancellationAdmissionReads >= configuration.cancellationAdmissionAfterReads) {
        if (unrelatedActive) {
          settled += 1;
          unrelatedActive = false;
          metrics.push(metric(requestId(999), "succeeded", 200, usage()));
        }
        pendingCancellation.admitted = true;
        accepted += 1;
      }
      if (!pendingCancellation
        || pendingCancellation.observationId !== observationId
        || !pendingCancellation.admitted) {
        return new Response(JSON.stringify({
          error: { code: "admission_not_found" },
        }), { status: 404 });
      }
      const body = {
        schema_version: 1,
        observation_id: observationId,
        status: configuration.admissionStatus ?? "active",
        request_id: pendingCancellation.requestId,
      };
      if (configuration.invalidAdmissionResponse) {
        body.request_id = "invalid";
      }
      return new Response(JSON.stringify(body), { status: 200 });
    }
    assert.equal(options.headers.authorization, `Bearer ${TOKEN}`);
    completions += 1;
    const index = completions;
    const internalRequestId = requestId(index);
    const request = JSON.parse(options.body);
    const sequence = request.frontier.parameters.properties.sequence.const;
    if (index === configuration.hangAtCompletion) {
      accepted += 1;
      return new Promise((resolvePromise, rejectPromise) => {
        const abort = () => {
          hungRequestAborted = true;
          settled += 1;
          metrics.push(metric(internalRequestId, "failed", 499, null));
          rejectPromise(new DOMException("aborted", "AbortError"));
        };
        if (options.signal.aborted) {
          abort();
        } else {
          options.signal.addEventListener("abort", abort, { once: true });
          configuration.onHungRequestStarted?.();
        }
      });
    }
    if (index === 14 && configuration.cancellationRejected) {
      return new Response(JSON.stringify({
        error: { code: "observation_registry_full" },
      }), { status: 503 });
    }
    accepted += 1;
    if (index === 14 && !configuration.cancellationCompletes) {
      const delayedAdmission = configuration.cancellationNeverAdmitted
        || Number.isSafeInteger(configuration.cancellationAdmissionAfterReads);
      if (delayedAdmission) {
        accepted -= 1;
      }
      const observationId = options.headers["x-starring-observation-id"];
      assert.match(
        observationId,
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      );
      pendingCancellation = {
        admitted: !delayedAdmission,
        observationId,
        requestId: internalRequestId,
      };
      return new Promise((resolvePromise, rejectPromise) => {
        const abort = () => {
          if (pendingCancellation.admitted) {
            settled += 1;
            metrics.push(metric(internalRequestId, "failed", 499, null));
          } else {
            cancellationAbortedBeforeAdmission = true;
          }
          rejectPromise(new DOMException("aborted", "AbortError"));
        };
        if (options.signal.aborted) {
          abort();
        } else {
          options.signal.addEventListener("abort", abort, { once: true });
        }
      });
    }
    settled += 1;
    metrics.push(metric(internalRequestId, "succeeded", 200, usage()));
    return new Response(JSON.stringify({
      schema_version: 1,
      request_id: internalRequestId,
      ...EXPECTED_IDENTITY,
      tool_call: {
        id: `call-${internalRequestId}`,
        name: "record_slo_probe",
        arguments: JSON.stringify({ schema_version: 1, sequence, status: "ok" }),
      },
      usage: usage(),
      duration_ms: 1,
    }), { status: 200 });
  };
  return {
    fetchFn,
    metrics,
    counters: () => ({ accepted, settled, completions }),
    state: () => ({
      cancellationAdmissionReads,
      cancellationAbortedBeforeAdmission,
      hungRequestAborted,
      unrelatedActive,
    }),
  };
}

test("live canary executes exactly fifteen calls without retries or secret artifacts", async () => {
  const fixture = liveFixture({ cancellationAdmissionAfterReads: 3 });
  let metricsContext = null;
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-test",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 100,
    healthPollMs: 1,
    metricsReader: async (context) => {
      metricsContext = context;
      return fixture.metrics;
    },
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, false);
  assert.equal(raw.observations.length, 15);
  assert.equal(raw.observed_live_calls, 15);
  assert.equal(raw.automatic_retries, 0);
  assert.equal(raw.worker_metrics.length, 15);
  assert.equal(raw.metrics_health_samples[0].stage, "initial");
  assert.equal(raw.metrics_health_samples.at(-1).records_written, 15);
  assert.equal(metricsContext.expected_records, 15);
  assert.equal(metricsContext.metrics_health_start.records_written, 0);
  assert.equal(metricsContext.metrics_health_end.records_written, 15);
  assert.equal(metricsContext.signal.aborted, false);
  assert.deepEqual(fixture.counters(), { accepted: 15, settled: 15, completions: 15 });
  assert.equal(fixture.state().cancellationAdmissionReads, 3);
  assert.equal(fixture.state().cancellationAbortedBeforeAdmission, false);
  assert.equal(raw.observations[13].outcome, "cancelled");
  assert.equal(raw.observations[13].request_id, requestId(14));
  assert.deepEqual(raw.observations[13].request_ids, [requestId(14)]);
  assert.equal(raw.observations[13].cancellation_recovered, true);
  const serialized = JSON.stringify(raw);
  assert.equal(serialized.includes(TOKEN), false);
  assert.equal(serialized.includes("authorization"), false);
  assert.equal(serialized.includes("messages"), false);
});

test("cancellation ignores an unrelated admitted request and aborts only its correlated request", async () => {
  const fixture = liveFixture({
    cancellationAdmissionAfterReads: 3,
    unrelatedAdmissionAfterRead: 1,
  });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-unrelated-race",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 100,
    healthPollMs: 1,
    metricsReader: async (context) => fixture.metrics.filter(
      (row) => context.request_ids.includes(row.request_id),
    ),
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, false);
  assert.equal(raw.observations[13].outcome, "cancelled");
  assert.equal(raw.observations[13].request_id, requestId(14));
  assert.deepEqual(raw.observations[13].request_ids, [requestId(14)]);
  assert.equal(raw.worker_metrics.some((row) => row.request_id === requestId(999)), false);
  assert.equal(raw.worker_metrics.some((row) => row.request_id === requestId(14)), true);
  assert.deepEqual(fixture.counters(), { accepted: 16, settled: 16, completions: 15 });
  assert.equal(fixture.state().cancellationAdmissionReads, 3);
  assert.equal(fixture.state().cancellationAbortedBeforeAdmission, false);
  assert.equal(fixture.state().unrelatedActive, false);
});

test("cancellation fails closed when the request is never admitted", async () => {
  const fixture = liveFixture({ cancellationNeverAdmitted: true });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-never-admitted",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 15,
    healthPollMs: 1,
    metricsHealthTimeoutMs: 20,
    metricsReader: async () => fixture.metrics,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "cancellation_not_admitted");
  assert.equal(raw.observations.length, 14);
  assert.equal(raw.observed_live_calls, 14);
  assert.equal(raw.automatic_retries, 0);
  assert.deepEqual(fixture.counters(), { accepted: 13, settled: 13, completions: 14 });
  assert.equal(fixture.state().cancellationAbortedBeforeAdmission, true);
  assert.equal(raw.observations.at(-1).outcome, "failed");
  assert.equal(raw.observations.at(-1).expected_outcome_met, false);
  assert.equal(JSON.stringify(raw).includes(TOKEN), false);
});

test("cancellation rejects a malformed correlated admission response and cleans up the request", async () => {
  const fixture = liveFixture({
    cancellationAdmissionAfterReads: 1,
    invalidAdmissionResponse: true,
  });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-invalid-admission",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 100,
    healthPollMs: 1,
    metricsReader: async () => fixture.metrics,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "admission_response_invalid");
  assert.equal(raw.observations.length, 14);
  assert.equal(raw.observations.at(-1).error_code, "admission_response_invalid");
  assert.deepEqual(raw.observations.at(-1).request_ids, []);
  assert.deepEqual(fixture.counters(), { accepted: 14, settled: 14, completions: 14 });
  assert.equal(fixture.state().cancellationAbortedBeforeAdmission, false);
});

test("cancellation rejects its correlated request when admission is only queued", async () => {
  const fixture = liveFixture({ admissionStatus: "queued" });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-queued-admission",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 100,
    healthPollMs: 1,
    metricsReader: async () => fixture.metrics,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "cancellation_not_active");
  assert.equal(raw.observations.length, 14);
  assert.equal(raw.observations.at(-1).error_code, "cancellation_not_active");
  assert.equal(raw.observations.at(-1).request_id, requestId(14));
  assert.deepEqual(raw.observations.at(-1).request_ids, [requestId(14)]);
  assert.deepEqual(fixture.counters(), { accepted: 14, settled: 14, completions: 14 });
  assert.equal(fixture.state().cancellationAbortedBeforeAdmission, false);
});

test("a cancellation request that completes preserves its real response evidence", async () => {
  const fixture = liveFixture({ cancellationCompletes: true });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-completed-before-cancel",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 100,
    healthPollMs: 1,
    metricsReader: async () => fixture.metrics,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  const cancellation = raw.observations.at(-1);
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "cancellation_not_observed");
  assert.equal(raw.observations.length, 14);
  assert.equal(cancellation.outcome, "completed");
  assert.equal(cancellation.correct, false);
  assert.equal(cancellation.request_id, requestId(14));
  assert.deepEqual(cancellation.request_ids, [requestId(14)]);
  assert.equal(cancellation.calls[0].request_id, requestId(14));
  assert.deepEqual(cancellation.usage, usage());
  assert.equal(cancellation.request_id.startsWith("fake-"), false);
});

test("a rejected cancellation response never invents a request id", async () => {
  const fixture = liveFixture({ cancellationRejected: true });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-rejected-before-admission",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    cancellationAdmissionTimeoutMs: 100,
    healthPollMs: 1,
    metricsHealthTimeoutMs: 20,
    metricsReader: async () => fixture.metrics,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  const cancellation = raw.observations.at(-1);
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "observation_registry_full");
  assert.equal(raw.observations.length, 14);
  assert.equal(cancellation.outcome, "failed");
  assert.equal(cancellation.request_id, null);
  assert.deepEqual(cancellation.request_ids, []);
  assert.equal(JSON.stringify(cancellation).includes("fake-"), false);
  assert.deepEqual(fixture.counters(), { accepted: 13, settled: 13, completions: 14 });
});

test("overall deadline aborts a hung fetch and stops all later waves", async () => {
  let triggerDeadline = () => {};
  const fixture = liveFixture({
    hangAtCompletion: 1,
    onHungRequestStarted: () => triggerDeadline(),
  });
  const plan = getPlan("live_canary");
  let scheduledDuration = null;
  const raw = await runPlan({
    plan,
    source: SOURCE,
    runId: "canary-run-deadline",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    healthPollMs: 1,
    metricsHealthTimeoutMs: 5,
    metricsReader: async () => fixture.metrics,
    scheduleRunDeadline: (controller, milliseconds) => {
      scheduledDuration = milliseconds;
      triggerDeadline = () => controller.abort();
      return () => {
        triggerDeadline = () => {};
      };
    },
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(scheduledDuration, plan.budgets.max_duration_ms);
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "duration_budget_exceeded");
  assert.equal(raw.observations.length, 1);
  assert.equal(raw.observed_live_calls, 1);
  assert.equal(raw.automatic_retries, 0);
  assert.equal(fixture.counters().completions, 1);
  assert.equal(fixture.state().hungRequestAborted, true);
  assert.equal(raw.observations[0].error_code, "duration_budget_exceeded");
  assert.equal(JSON.stringify(raw).includes(TOKEN), false);
});

test("a hung resource sampler start is aborted and still receives bounded cleanup", async () => {
  let triggerDeadline = () => {};
  let startSignal = null;
  let cleanupSignal = null;
  let cleanupCalls = 0;
  const raw = await runPlan({
    plan: getPlan("development"),
    source: SOURCE,
    runId: "development-resource-start-timeout",
    resourceSampler: {
      start(signal) {
        startSignal = signal;
        triggerDeadline();
        return new Promise(() => {});
      },
      async stop(signal) {
        cleanupCalls += 1;
        cleanupSignal = signal;
        return { samples: [], errors: [], duration_ms: 0 };
      },
    },
    scheduleRunDeadline: (controller) => {
      triggerDeadline = () => controller.abort();
      return () => {
        triggerDeadline = () => {};
      };
    },
    resourceCleanupTimeoutMs: 20,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "duration_budget_exceeded");
  assert.equal(raw.observations.length, 0);
  assert.equal(cleanupCalls, 1);
  assert.equal(startSignal.aborted, true);
  assert.notEqual(cleanupSignal, startSignal);
  assert.equal(cleanupSignal.aborted, false);
});

test("a hung resource sampler stop is bounded by its independent cleanup signal", async () => {
  let startSignal = null;
  let cleanupSignal = null;
  const raw = await runPlan({
    plan: getPlan("development"),
    source: SOURCE,
    runId: "development-resource-stop-timeout",
    resourceSampler: {
      async start(signal) {
        startSignal = signal;
      },
      stop(signal) {
        cleanupSignal = signal;
        return new Promise(() => {});
      },
    },
    resourceCleanupTimeoutMs: 10,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "resource_sampler_cleanup_timeout");
  assert.equal(raw.observations.length, 12);
  assert.notEqual(cleanupSignal, startSignal);
  assert.equal(startSignal.aborted, false);
  assert.equal(cleanupSignal.aborted, true);
});

test("first unexpected wave outcome stops later waves and never retries", async () => {
  let calls = 0;
  const plan = getPlan("development");
  const raw = await runPlan({
    plan,
    source: SOURCE,
    runId: "development-fail-fast",
    fakeExecutor: async (_workload, sequence) => {
      calls += 1;
      if (calls === 1) {
        const error = new Error("bounded");
        error.code = "fake_failure";
        throw error;
      }
      return {
        outcome: "completed",
        correct: true,
        expected_outcome_met: true,
        status_code: 200,
        error_code: null,
        request_id: `fake-${sequence}`,
        ...plan.identity,
        frontier_name: "record_slo_probe",
        worker_duration_ms: 0,
        calls: [{
          status_code: 200,
          latency_ms: 0,
          ...plan.identity,
          frontier_name: "record_slo_probe",
          usage: usage(0, 0),
        }],
        usage: usage(0, 0),
        product: null,
      };
    },
    healthPollMs: 1,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "fake_failure");
  assert.equal(raw.observations.length, 4);
  assert.equal(calls, 4);
  assert.equal(raw.automatic_retries, 0);
  assert.equal(raw.observed_live_calls, 0);
  assert.equal(raw.counters.end_accepted - raw.counters.start_accepted, 4);
});

test("degraded metrics health stops before any live completion", async () => {
  const fixture = liveFixture({ metricsStatus: "degraded", metricsFailures: 1 });
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: SOURCE,
    runId: "canary-metrics-degraded",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    metricsReader: async () => [],
    healthPollMs: 1,
    metricsHealthTimeoutMs: 5,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "metrics_unavailable");
  assert.equal(raw.observations.length, 0);
  assert.equal(fixture.counters().completions, 0);
});

test("stale worker source stops before any live completion", async () => {
  const fixture = liveFixture();
  const raw = await runPlan({
    plan: getPlan("live_canary"),
    source: { ...SOURCE, worker_source_sha256: "c".repeat(64) },
    runId: "canary-stale-worker",
    baseUrl: "http://127.0.0.1:18181",
    token: TOKEN,
    fetchFn: fixture.fetchFn,
    metricsReader: async () => [],
    healthPollMs: 1,
    metricsHealthTimeoutMs: 5,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  assert.equal(raw.interrupted, true);
  assert.equal(raw.stop_reason, "worker_source_mismatch");
  assert.equal(raw.observations.length, 0);
  assert.equal(fixture.counters().completions, 0);
});

test("real worker and runner correlate active cancellation through its terminal metric", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-worker-integration-"));
  const metricsPath = join(directory, "metrics", "worker.jsonl");
  const runner = {
    verify: async () => ({
      codex_cli_version: EXPECTED_IDENTITY.codex_cli_version,
      auth_mode: EXPECTED_IDENTITY.auth_mode,
    }),
    complete: async ({ frontier, signal }) => {
      await new Promise((resolvePromise, rejectPromise) => {
        const complete = () => {
          signal.removeEventListener("abort", abort);
          resolvePromise();
        };
        const timer = setTimeout(complete, 40);
        const abort = () => {
          clearTimeout(timer);
          rejectPromise(signal.reason);
        };
        if (signal.aborted) {
          abort();
        } else {
          signal.addEventListener("abort", abort, { once: true });
        }
      });
      return {
        model: EXPECTED_IDENTITY.model,
        reasoning_effort: EXPECTED_IDENTITY.reasoning_effort,
        auth_mode: EXPECTED_IDENTITY.auth_mode,
        codex_cli_version: EXPECTED_IDENTITY.codex_cli_version,
        arguments: JSON.stringify({
          schema_version: 1,
          sequence: frontier.parameters.properties.sequence.const,
          status: "ok",
        }),
        usage: usage(),
      };
    },
  };
  const worker = await startWorker({
    token: TOKEN,
    port: 0,
    runner,
    metricsPath,
    concurrency: 2,
    maxQueue: 8,
    timeoutMs: 55_000,
    instanceId: "slo-integration-worker",
    workerSourceSha256: SOURCE.worker_source_sha256,
  });
  try {
    const baseUrl = `http://${worker.address.address}:${worker.address.port}`;
    const metricsReader = await createFileMetricsReader({ path: metricsPath });
    const raw = await runPlan({
      plan: getPlan("live_canary"),
      source: SOURCE,
      runId: "canary-real-worker-integration",
      baseUrl,
      token: TOKEN,
      metricsReader,
      healthPollMs: 1,
      cancellationAdmissionTimeoutMs: 1_000,
      metricsHealthTimeoutMs: 1_000,
      wallClock: () => "2026-07-17T00:00:00.000Z",
    });
    raw.source_end = structuredClone(SOURCE);
    const cancellation = raw.observations.find(
      (row) => row.expected_outcome === "cancelled",
    );
    const cancellationMetric = raw.worker_metrics.find(
      (metricRow) => metricRow.request_id === cancellation.request_id,
    );
    assert.equal(raw.interrupted, false);
    assert.equal(cancellation.outcome, "cancelled");
    assert.equal(cancellation.request_id, cancellationMetric.request_id);
    assert.equal(cancellationMetric.status_code, 499);
    assert.equal(cancellationMetric.error_code, "client_disconnected");
    const acceptance = assessRun(raw.plan, raw, summarizeRun(raw));
    assert.equal(
      acceptance.gates.find((entry) => entry.name === "worker_metric_correlation").status,
      "pass",
    );
    assert.equal(acceptance.verdict, "pass");
  } finally {
    worker.server.closeIdleConnections();
    await worker.close();
    await rm(directory, { recursive: true, force: true });
  }
});
