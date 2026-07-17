import assert from "node:assert/strict";
import test from "node:test";
import { assessRun } from "./acceptance.mjs";
import { EXPECTED_IDENTITY, getPlan, planDigest } from "./plans.mjs";
import { summarizeRun } from "./summarize.mjs";

function usage() {
  return {
    input_tokens: 1,
    cached_input_tokens: 0,
    output_tokens: 1,
    reasoning_output_tokens: 0,
  };
}

function workerMetric(requestId, outcome, statusCode) {
  return {
    metric_schema_version: 2,
    timestamp: "2026-07-17T00:00:00.000Z",
    request_id: requestId,
    ...EXPECTED_IDENTITY,
    frontier_name: "record_slo_probe",
    instance_id: "acceptance-instance",
    worker_source_sha256: "b".repeat(64),
    concurrency_limit: 2,
    queue_capacity: 8,
    request_timeout_ms: 55_000,
    active_at_admission: 0,
    queued_at_admission: 0,
    queue_wait_ms: 0,
    runner_duration_ms: 90,
    runner_elapsed_at_terminal_ms: null,
    post_runner_ms: 10,
    total_duration_ms: 100,
    runner_started: true,
    runner_settled: true,
    runner_outcome: outcome === "succeeded" ? "resolved" : "rejected",
    result_validation_started: outcome === "succeeded",
    terminal_stage: outcome === "succeeded" ? "completed" : "runner",
    outcome,
    status_code: statusCode,
    duration_ms: 100,
    usage: outcome === "succeeded" ? usage() : null,
    error_code: outcome === "succeeded" ? null : "client_disconnected",
  };
}

function canaryRaw() {
  const plan = getPlan("live_canary");
  const observations = [];
  const waves = [];
  const metrics = [];
  let sequence = 0;
  let offset = 0;
  for (const phase of plan.phases) {
    for (let waveIndex = 0; waveIndex < phase.waves; waveIndex += 1) {
      const start = offset;
      for (let slotIndex = 0; slotIndex < phase.concurrency; slotIndex += 1) {
        sequence += 1;
        const cancelled = phase.expected_outcome === "cancelled";
        const requestId = `request-${sequence}`;
        const metricId = requestId;
        observations.push({
          schema_version: 1,
          sequence: `acceptance-${sequence}`,
          phase_id: phase.id,
          workload_id: phase.workload_id,
          wave_index: waveIndex,
          slot_index: slotIndex,
          concurrency: phase.concurrency,
          warmup: phase.warmup,
          expected_outcome: phase.expected_outcome,
          attempt: 1,
          provider_invoked: true,
          planned_calls: 1,
          dispatch_offset_ms: offset,
          completion_offset_ms: offset + 100,
          latency_ms: 100,
          outcome: cancelled ? "cancelled" : "completed",
          correct: !cancelled,
          expected_outcome_met: true,
          status_code: cancelled ? 499 : 200,
          error_code: cancelled ? "client_disconnected" : null,
          request_id: requestId,
          request_ids: [requestId],
          provider: cancelled ? null : EXPECTED_IDENTITY.provider,
          model: cancelled ? null : EXPECTED_IDENTITY.model,
          reasoning_effort: cancelled ? null : EXPECTED_IDENTITY.reasoning_effort,
          auth_mode: cancelled ? null : EXPECTED_IDENTITY.auth_mode,
          codex_cli_version: cancelled ? null : EXPECTED_IDENTITY.codex_cli_version,
          frontier_name: "record_slo_probe",
          worker_duration_ms: cancelled ? null : 100,
          calls: cancelled ? [] : [{
            request_id: requestId,
            status_code: 200,
            latency_ms: 100,
            ...EXPECTED_IDENTITY,
            frontier_name: "record_slo_probe",
            usage: usage(),
          }],
          usage: cancelled ? {
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
          } : usage(),
          product: null,
          ...(cancelled ? {
            cancellation_recovery_ms: 100,
            cancellation_recovered: true,
          } : {}),
        });
        metrics.push(workerMetric(metricId, cancelled ? "failed" : "succeeded", cancelled ? 499 : 200));
      }
      const duration = phase.id === "parallel_two" ? 1_000 : 1_000;
      offset += duration;
      waves.push({
        phase_id: phase.id,
        workload_id: phase.workload_id,
        wave_index: waveIndex,
        concurrency: phase.concurrency,
        start_offset_ms: start,
        end_offset_ms: offset,
        duration_ms: duration,
        observations: phase.concurrency,
        correct_completions: phase.expected_outcome === "cancelled" ? 0 : phase.concurrency,
        expected_outcomes_met: phase.concurrency,
      });
    }
  }
  const boundary = {
    instance_id: "acceptance-instance",
    worker_source_sha256: "b".repeat(64),
    identity: structuredClone(EXPECTED_IDENTITY),
    profile: structuredClone(plan.worker_profile),
  };
  const health = (stage, accepted) => ({
    at_ms: stage === "initial" ? 0 : offset,
    stage,
    schema_version: 1,
    status: "ok",
    ...EXPECTED_IDENTITY,
    instance_id: boundary.instance_id,
    worker_source_sha256: boundary.worker_source_sha256,
    ...plan.worker_profile,
    active_requests: 0,
    queued_requests: 0,
    accepted_requests_total: accepted,
    settled_requests_total: accepted,
  });
  return {
    schema_version: 1,
    run_id: "acceptance-canary",
    plan,
    plan_digest: planDigest(plan),
    source: {
      commit: "a".repeat(40),
      dirty: false,
      worker_source_sha256: "b".repeat(64),
    },
    source_end: {
      commit: "a".repeat(40),
      dirty: false,
      worker_source_sha256: "b".repeat(64),
    },
    execution_mode: "live",
    started_at: "2026-07-17T00:00:00.000Z",
    completed_at: "2026-07-17T00:01:00.000Z",
    duration_ms: offset,
    interrupted: false,
    stop_reason: null,
    automatic_retries: 0,
    planned_live_calls: 15,
    observed_live_calls: 15,
    live_call_count_known: true,
    usage: {
      input_tokens: 14,
      cached_input_tokens: 0,
      output_tokens: 14,
      reasoning_output_tokens: 0,
    },
    worker_boundary: boundary,
    counters: {
      start_accepted: 0,
      start_settled: 0,
      end_accepted: 15,
      end_settled: 15,
    },
    health_samples: [health("initial", 0), health("final", 15)],
    metrics_health_samples: [
      {
        at_ms: 0,
        stage: "initial",
        schema_version: 1,
        instance_id: boundary.instance_id,
        worker_source_sha256: boundary.worker_source_sha256,
        status: "ok",
        writable_verified: true,
        records_attempted: 0,
        records_written: 0,
        pending_records: 0,
        write_failures_total: 0,
        last_error_code: null,
      },
      {
        at_ms: offset,
        stage: "final",
        schema_version: 1,
        instance_id: boundary.instance_id,
        worker_source_sha256: boundary.worker_source_sha256,
        status: "ok",
        writable_verified: true,
        records_attempted: 15,
        records_written: 15,
        pending_records: 0,
        write_failures_total: 0,
        last_error_code: null,
      },
    ],
    resource_samples: [],
    resource_errors: [],
    resource_duration_ms: 0,
    worker_metrics: metrics,
    scenarios: [{ id: "cancellation", status: "pass", duration_ms: 100, error_code: null }],
    waves,
    observations,
  };
}

test("clean canary is diagnostic-only and promotes step load", () => {
  const raw = canaryRaw();
  const summary = summarizeRun(raw);
  const acceptance = assessRun(raw.plan, raw, summary);
  assert.equal(acceptance.verdict, "pass");
  assert.equal(acceptance.claims.diagnostic_complete, true);
  assert.equal(acceptance.claims.eligible_for_step_load, true);
  assert.equal(acceptance.claims.commercial_slo_certified, false);
  assert.equal(acceptance.claims.availability_24h_supported, false);
  assert.ok(acceptance.non_claims.includes("commercial_slo_not_certified"));
  assert.ok(acceptance.non_claims.includes("annual_availability_not_inferred"));
  assert.ok(acceptance.non_claims.includes("cancelled_request_token_usage_unobserved"));
});

test("acceptance rejects summaries that were not derived from raw evidence", () => {
  const raw = canaryRaw();
  const summary = summarizeRun(raw);
  summary.latency.serial.maximum_ms = 0;
  assert.throws(
    () => assessRun(raw.plan, raw, summary),
    /summary_run_mismatch/,
  );
});

test("missing metric correlation fails closed", () => {
  const raw = canaryRaw();
  raw.worker_metrics.pop();
  const summary = summarizeRun(raw);
  const acceptance = assessRun(raw.plan, raw, summary);
  const metricGate = acceptance.gates.find((entry) => entry.name === "worker_metric_correlation");
  assert.equal(metricGate.status, "fail");
  assert.equal(acceptance.verdict, "fail");
  assert.equal(acceptance.claims.eligible_for_step_load, false);
});

test("impossible timing decomposition and late unsettled runners fail closed", () => {
  const decomposed = canaryRaw();
  decomposed.worker_metrics[0].total_duration_ms = 101;
  let acceptance = assessRun(decomposed.plan, decomposed, summarizeRun(decomposed));
  assert.equal(
    acceptance.gates.find((entry) => entry.name === "worker_metric_correlation").status,
    "fail",
  );

  const unsettled = canaryRaw();
  const cancellation = unsettled.worker_metrics.at(-2);
  cancellation.runner_settled = false;
  cancellation.runner_duration_ms = null;
  cancellation.runner_elapsed_at_terminal_ms = 100;
  cancellation.post_runner_ms = null;
  cancellation.runner_outcome = null;
  acceptance = assessRun(unsettled.plan, unsettled, summarizeRun(unsettled));
  assert.equal(
    acceptance.gates.find((entry) => entry.name === "worker_metric_correlation").status,
    "fail",
  );
});

test("metrics health degradation and pending writes fail closed", () => {
  const raw = canaryRaw();
  raw.metrics_health_samples.at(-1).pending_records = 1;
  const acceptance = assessRun(raw.plan, raw, summarizeRun(raw));
  assert.equal(
    acceptance.gates.find((entry) => entry.name === "metrics_health_continuity").status,
    "fail",
  );
  assert.equal(acceptance.claims.eligible_for_step_load, false);
});

test("cancellation recovery requires one measured duration", () => {
  const raw = canaryRaw();
  delete raw.observations.find((row) => row.expected_outcome === "cancelled")
    .cancellation_recovery_ms;
  const acceptance = assessRun(raw.plan, raw, summarizeRun(raw));
  assert.equal(
    acceptance.gates.find((entry) => entry.name === "cancellation_recovery").status,
    "fail",
  );
  assert.equal(acceptance.claims.eligible_for_step_load, false);
});

test("stale worker source cannot promote a clean canary", () => {
  const raw = canaryRaw();
  raw.source.worker_source_sha256 = "c".repeat(64);
  raw.source_end.worker_source_sha256 = "c".repeat(64);
  const acceptance = assessRun(raw.plan, raw, summarizeRun(raw));
  assert.equal(
    acceptance.gates.find(
      (entry) => entry.name === "worker_source_bound_to_clean_commit",
    ).status,
    "fail",
  );
  assert.equal(acceptance.claims.eligible_for_step_load, false);
});

test("response metrics and aggregate usage require an exact join", () => {
  const latency = canaryRaw();
  latency.worker_metrics[0].duration_ms += 1;
  let acceptance = assessRun(latency.plan, latency, summarizeRun(latency));
  assert.equal(
    acceptance.gates.find((entry) => entry.name === "worker_metric_correlation").status,
    "fail",
  );

  const usageDrift = canaryRaw();
  usageDrift.usage.input_tokens += 1;
  acceptance = assessRun(usageDrift.plan, usageDrift, summarizeRun(usageDrift));
  assert.equal(
    acceptance.gates.find((entry) => entry.name === "worker_metric_correlation").status,
    "fail",
  );
});
