import assert from "node:assert/strict";
import test from "node:test";
import { runPlan } from "./load-runner.mjs";
import { getPlan } from "./plans.mjs";
import { nearestRank, summarizeRun } from "./summarize.mjs";

function productObservation({ sequence, phaseId, latencyMs, callLatencies }) {
  return {
    sequence,
    phase_id: phaseId,
    workload_id: "starring_v15_two_call",
    warmup: false,
    expected_outcome: "completed",
    expected_outcome_met: true,
    outcome: "completed",
    correct: true,
    planned_calls: callLatencies.length,
    latency_ms: latencyMs,
    request_id: null,
    calls: callLatencies.map((latency, index) => ({
      request_id: `${sequence}-request-${index + 1}`,
      latency_ms: latency,
    })),
    product: {
      exact_semantics: true,
      validation_current: true,
      simulation_current: true,
      candidate_only: true,
    },
  };
}

function productRaw() {
  const observations = [
    productObservation({
      sequence: "serial-1",
      phaseId: "serial",
      latencyMs: 100,
      callLatencies: [40],
    }),
    productObservation({
      sequence: "parallel-1",
      phaseId: "parallel_two",
      latencyMs: 80,
      callLatencies: [20, 30],
    }),
    productObservation({
      sequence: "parallel-2",
      phaseId: "parallel_two",
      latencyMs: 90,
      callLatencies: [25, 35],
    }),
  ];
  return {
    schema_version: 1,
    run_id: "product-summary",
    plan: {
      id: "summary_fixture",
      revision: 1,
      claim_scope: "test_only",
      phases: [
        { concurrency: 1, waves: 1 },
        { concurrency: 2, waves: 1 },
      ],
    },
    execution_mode: "live",
    interrupted: false,
    stop_reason: null,
    duration_ms: 200,
    observations,
    waves: [
      { phase_id: "serial", duration_ms: 100 },
      { phase_id: "parallel_two", duration_ms: 100 },
    ],
    health_samples: [{ active_requests: 0, queued_requests: 0 }],
    metrics_health_samples: [],
    resource_samples: [{
      rss_bytes: 1,
      heap_used_bytes: 1,
      cpu_percent: 0,
      evaluator_event_loop_delay_p99_ms: 0,
    }],
    resource_errors: [],
    resource_duration_ms: 200,
    worker_metrics: [],
    usage: {
      input_tokens: 0,
      cached_input_tokens: 0,
      output_tokens: 0,
      reasoning_output_tokens: 0,
    },
    observed_live_calls: 5,
    live_call_count_known: true,
    automatic_retries: 0,
    counters: {
      start_accepted: 0,
      start_settled: 0,
      end_accepted: 5,
      end_settled: 5,
    },
  };
}

test("nearest-rank percentiles are exact and do not interpolate", () => {
  assert.equal(nearestRank([4, 1, 3, 2], 0.5), 2);
  assert.equal(nearestRank([4, 1, 3, 2], 0.95), 4);
  assert.equal(nearestRank([4, 1, 3, 2], 0.99), 4);
  assert.equal(nearestRank([], 0.95), null);
});

test("summary retains failures in the denominator and excludes warmup latency", async () => {
  const raw = await runPlan({
    plan: getPlan("development"),
    source: { commit: "a".repeat(40), dirty: false },
    runId: "summary-development",
    healthPollMs: 1,
    wallClock: () => "2026-07-17T00:00:00.000Z",
  });
  raw.observations[0].outcome = "failed";
  raw.observations[0].correct = false;
  raw.observations[0].expected_outcome_met = false;
  raw.observations[0].error_code = "bounded_failure";
  const summary = summarizeRun(raw);
  assert.equal(summary.counts.planned_attempts, 12);
  assert.equal(summary.counts.correct_attempts, 11);
  assert.equal(summary.counts.unexpected_attempts, 1);
  assert.equal(summary.rates.unexpected_error, 1 / 12);
  assert.equal(summary.latency.successful_requests.count, 11);
  assert.deepEqual(summary.errors.by_code, { bounded_failure: 1 });
  assert.equal(summary.counters.accepted_delta, 12);
  assert.equal(summary.counters.settled_delta, 12);
});

test("summary separates end-to-end invocation and model-call SLIs", () => {
  const summary = summarizeRun(productRaw());
  assert.equal(summary.latency.end_to_end_invocations.successful.count, 3);
  assert.equal(summary.latency.end_to_end_invocations.serial.maximum_ms, 100);
  assert.equal(summary.latency.model_calls.successful.count, 5);
  assert.equal(summary.latency.model_calls.serial.maximum_ms, 40);
  assert.equal(
    summary.throughput.end_to_end_invocations.serial.invocations_per_second,
    10,
  );
  assert.equal(
    summary.throughput.end_to_end_invocations.parallel_two.invocations_per_second,
    20,
  );
  assert.equal(summary.throughput.end_to_end_invocations.parallel_to_serial_ratio, 2);
  assert.equal(summary.throughput.model_calls.serial.model_calls_per_second, 10);
  assert.equal(summary.throughput.model_calls.parallel_two.model_calls_per_second, 40);
  assert.equal(summary.throughput.model_calls.parallel_to_serial_ratio, 4);
  assert.equal(summary.latency.successful_requests.count, 3);
  assert.equal(summary.throughput.parallel_to_serial_ratio, 2);
  assert.equal(summary.worker_timing.expected_records, 5);
});

test("zero-token product evidence remains zero instead of missing", () => {
  const summary = summarizeRun(productRaw());
  assert.equal(summary.usage.input_tokens, 0);
  assert.equal(summary.usage.output_tokens, 0);
  assert.equal(summary.usage.mean_input_tokens_per_correct_invocation, 0);
  assert.equal(summary.usage.mean_output_tokens_per_correct_invocation, 0);
  assert.equal(summary.usage.mean_input_tokens_per_correct_model_call, 0);
  assert.equal(summary.usage.mean_output_tokens_per_correct_model_call, 0);
  assert.deepEqual(summary.resources.evaluator_event_loop_delay_p99_ms, {
    minimum: 0,
    maximum: 0,
    mean: 0,
  });
});
