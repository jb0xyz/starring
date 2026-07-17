import {
  assertKnownPlan,
  canonicalJson,
  planDigest,
  planInvocationCount,
  planLiveCallCount,
} from "./plans.mjs";
import { summarizeRun } from "./summarize.mjs";

const FORBIDDEN_LIVE_ERRORS = new Set([
  "chatgpt_login_required",
  "codex_exit_failed",
  "codex_timeout",
  "identity_mismatch",
  "invalid_codex_version",
  "invalid_structured_output",
  "metrics_unavailable",
  "provider_identity_mismatch",
  "quota_exhausted",
  "worker_boundary_mismatch",
  "worker_continuity_lost",
  "worker_source_mismatch",
]);

const METRIC_REQUIRED_FIELDS = [
  "metric_schema_version",
  "request_id",
  "instance_id",
  "worker_source_sha256",
  "concurrency_limit",
  "queue_capacity",
  "request_timeout_ms",
  "active_at_admission",
  "queued_at_admission",
  "queue_wait_ms",
  "runner_duration_ms",
  "runner_elapsed_at_terminal_ms",
  "post_runner_ms",
  "total_duration_ms",
  "runner_started",
  "runner_settled",
  "runner_outcome",
  "result_validation_started",
  "terminal_stage",
  "outcome",
  "status_code",
  "duration_ms",
  "usage",
  "error_code",
];

function gate(name, pass, actual, target, required = true, evaluated = true) {
  return {
    name,
    required,
    status: evaluated ? (pass ? "pass" : "fail") : "not_evaluated",
    actual,
    target,
  };
}

function allPass(gates) {
  return gates.every((entry) => !entry.required || entry.status === "pass");
}

function scenarioStatus(raw, id) {
  const scenario = (raw.scenarios ?? []).find((entry) => entry.id === id);
  return scenario?.status ?? null;
}

function healthContinuity(raw) {
  const boundary = raw.worker_boundary;
  return boundary !== null && raw.health_samples.every((sample) => (
    sample.instance_id === boundary.instance_id
      && sample.worker_source_sha256 === boundary.worker_source_sha256
      && Object.entries(boundary.identity).every(([key, value]) => sample[key] === value)
      && Object.entries(boundary.profile).every(([key, value]) => sample[key] === value)
  ));
}

function nonnegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function nonnegativeIntegerOrNull(value) {
  return value === null || nonnegativeInteger(value);
}

function validUsage(value) {
  return value
    && typeof value === "object"
    && [
      "input_tokens",
      "cached_input_tokens",
      "output_tokens",
      "reasoning_output_tokens",
    ].every((key) => nonnegativeInteger(value[key]))
    && value.cached_input_tokens <= value.input_tokens
    && value.reasoning_output_tokens <= value.output_tokens;
}

function sameUsage(left, right) {
  return validUsage(left)
    && validUsage(right)
    && [
      "input_tokens",
      "cached_input_tokens",
      "output_tokens",
      "reasoning_output_tokens",
    ].every((key) => left[key] === right[key]);
}

function addUsage(left, right) {
  return {
    input_tokens: left.input_tokens + right.input_tokens,
    cached_input_tokens: left.cached_input_tokens + right.cached_input_tokens,
    output_tokens: left.output_tokens + right.output_tokens,
    reasoning_output_tokens: left.reasoning_output_tokens + right.reasoning_output_tokens,
  };
}

function summedUsage(values) {
  return values.reduce((total, value) => addUsage(total, value), {
    input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
  });
}

function metricShape(metric, raw) {
  const successful = metric?.outcome === "success" || metric?.outcome === "succeeded";
  if (!metric
    || typeof metric !== "object"
    || Array.isArray(metric)
    || !METRIC_REQUIRED_FIELDS.every((key) => Object.hasOwn(metric, key))) {
    return false;
  }
  if (metric.metric_schema_version !== 2
    || metric.instance_id !== raw.worker_boundary?.instance_id
    || metric.worker_source_sha256 !== raw.worker_boundary?.worker_source_sha256
    || metric.provider !== raw.plan.identity.provider
    || metric.model !== raw.plan.identity.model
    || metric.reasoning_effort !== raw.plan.identity.reasoning_effort
    || metric.concurrency_limit !== raw.plan.worker_profile.concurrency_limit
    || metric.queue_capacity !== raw.plan.worker_profile.queue_capacity
    || metric.request_timeout_ms !== raw.plan.worker_profile.request_timeout_ms) {
    return false;
  }
  if (typeof metric.request_id !== "string" || metric.request_id.length === 0
    || typeof metric.timestamp !== "string"
    || Number.isNaN(Date.parse(metric.timestamp))
    || typeof metric.frontier_name !== "string"
    || !/^[a-z][a-z0-9_]{0,127}$/.test(metric.frontier_name)
    || !["succeeded", "failed"].includes(metric.outcome)
    || !Number.isSafeInteger(metric.status_code)
    || !nonnegativeInteger(metric.active_at_admission)
    || !nonnegativeInteger(metric.queued_at_admission)
    || metric.active_at_admission > raw.plan.worker_profile.concurrency_limit
    || metric.queued_at_admission > raw.plan.worker_profile.queue_capacity
    || !nonnegativeInteger(metric.queue_wait_ms)
    || !nonnegativeIntegerOrNull(metric.runner_duration_ms)
    || !nonnegativeIntegerOrNull(metric.runner_elapsed_at_terminal_ms)
    || !nonnegativeIntegerOrNull(metric.post_runner_ms)
    || !nonnegativeInteger(metric.total_duration_ms)
    || !nonnegativeInteger(metric.duration_ms)
    || typeof metric.runner_started !== "boolean"
    || typeof metric.runner_settled !== "boolean"
    || typeof metric.result_validation_started !== "boolean"
    || !["admission", "completed", "queue", "result_validation", "runner"].includes(metric.terminal_stage)
    || ![null, "resolved", "rejected"].includes(metric.runner_outcome)) {
    return false;
  }
  const runnerComponent = metric.runner_duration_ms ?? metric.runner_elapsed_at_terminal_ms ?? 0;
  const postComponent = metric.post_runner_ms ?? 0;
  if (metric.queue_wait_ms + runnerComponent + postComponent !== metric.total_duration_ms) {
    return false;
  }
  if (metric.runner_settled
    && (!metric.runner_started
      || metric.runner_duration_ms === null
      || metric.runner_elapsed_at_terminal_ms !== null
      || metric.post_runner_ms === null
      || !["resolved", "rejected"].includes(metric.runner_outcome))) {
    return false;
  }
  if (!metric.runner_settled
    && (metric.runner_duration_ms !== null || metric.runner_outcome !== null)) {
    return false;
  }
  if (metric.runner_started && !metric.runner_settled
    && (metric.runner_elapsed_at_terminal_ms === null || metric.post_runner_ms !== null)) {
    return false;
  }
  if (metric.runner_started && !metric.runner_settled) {
    return false;
  }
  if (!metric.runner_started
    && (metric.runner_settled
      || metric.runner_duration_ms !== null
      || metric.runner_elapsed_at_terminal_ms !== null
      || metric.runner_outcome !== null
      || metric.result_validation_started
      || !["admission", "queue"].includes(metric.terminal_stage))) {
    return false;
  }
  if (metric.terminal_stage === "admission"
    && (metric.queue_wait_ms !== 0 || metric.post_runner_ms !== metric.total_duration_ms)) {
    return false;
  }
  if (metric.terminal_stage === "queue"
    && (metric.queue_wait_ms !== metric.total_duration_ms || metric.post_runner_ms !== 0)) {
    return false;
  }
  if (metric.result_validation_started
    && (!metric.runner_settled
      || metric.runner_outcome !== "resolved"
      || !["completed", "result_validation"].includes(metric.terminal_stage))) {
    return false;
  }
  if (metric.runner_outcome === "rejected"
    && (metric.result_validation_started || metric.terminal_stage !== "runner")) {
    return false;
  }
  if (metric.runner_started && !metric.runner_settled && metric.terminal_stage !== "runner") {
    return false;
  }
  if (successful) {
    return metric.status_code === 200
      && metric.error_code === null
      && validUsage(metric.usage)
      && metric.runner_started
      && metric.runner_settled
      && metric.runner_outcome === "resolved"
      && metric.result_validation_started
      && metric.terminal_stage === "completed";
  }
  return metric.outcome === "failed"
    && metric.usage === null
    && metric.status_code >= 400
    && metric.status_code <= 599
    && typeof metric.error_code === "string"
    && metric.terminal_stage !== "completed";
}

function metricCorrelation(raw) {
  const metrics = Array.isArray(raw.worker_metrics) ? raw.worker_metrics : [];
  const responseEntries = raw.observations.flatMap((row) => {
    const calls = Array.isArray(row.calls) ? row.calls : [];
    if (calls.length > 0) {
      return calls.map((call, index) => ({
        row,
        call,
        request_id: call.request_id
          ?? (calls.length === 1 ? row.request_id : row.request_ids?.[index]),
        expected_outcome: "completed",
      }));
    }
    const requestIds = Array.isArray(row.request_ids)
      ? row.request_ids
      : typeof row.request_id === "string"
        ? [row.request_id]
        : [];
    return requestIds.map((requestId) => ({
      row,
      call: null,
      request_id: requestId,
      expected_outcome: row.expected_outcome,
    }));
  });
  const responseIds = responseEntries
    .map((entry) => entry.request_id)
    .filter((id) => typeof id === "string" && id.length > 0);
  const metricIds = metrics.map((metric) => metric.request_id);
  const uniqueMetrics = new Set(metricIds);
  const responseCovered = responseEntries.every((entry) => {
    const matches = metrics.filter((metric) => metric.request_id === entry.request_id);
    if (matches.length !== 1) {
      return false;
    }
    const metric = matches[0];
    if (entry.expected_outcome === "cancelled") {
      return metric.outcome === "failed"
        && metric.status_code === entry.row.status_code
        && metric.error_code === "client_disconnected"
        && metric.frontier_name === entry.row.frontier_name;
    }
    const call = entry.call;
    return call !== null
      && metric.outcome === "succeeded"
      && metric.status_code === call.status_code
      && metric.status_code === entry.row.status_code
      && metric.provider === call.provider
      && metric.model === call.model
      && metric.reasoning_effort === call.reasoning_effort
      && metric.frontier_name === call.frontier_name
      && metric.duration_ms === call.latency_ms
      && sameUsage(metric.usage, call.usage);
  });
  const expectedUnmatched = 0;
  const unmatchedMetrics = metrics.filter((metric) => !responseIds.includes(metric.request_id));
  const unmatched = unmatchedMetrics.length;
  const callUsage = summedUsage(raw.observations.flatMap(
    (row) => Array.isArray(row.calls) ? row.calls.map((call) => call.usage) : [],
  ));
  const metricUsage = summedUsage(metrics
    .filter((metric) => metric.outcome === "succeeded")
    .map((metric) => metric.usage));
  const observationUsageExact = raw.observations.every((row) => (
    Array.isArray(row.calls)
      && sameUsage(row.usage, summedUsage(row.calls.map((call) => call.usage)))
  ));
  const expectedRecords = Number.isSafeInteger(raw.counters.end_accepted)
    && Number.isSafeInteger(raw.counters.start_accepted)
    ? raw.counters.end_accepted - raw.counters.start_accepted
    : null;
  return {
    pass: expectedRecords !== null
      && metrics.length === expectedRecords
      && uniqueMetrics.size === metrics.length
      && new Set(responseIds).size === responseIds.length
      && metrics.every((metric) => metricShape(metric, raw))
      && responseCovered
      && unmatched === expectedUnmatched
      && observationUsageExact
      && sameUsage(raw.usage, callUsage)
      && sameUsage(raw.usage, metricUsage),
    actual: {
      records: metrics.length,
      expected_records: expectedRecords,
      unique_records: uniqueMetrics.size,
      response_ids: responseIds.length,
      unmatched_records: unmatched,
      expected_unmatched_records: expectedUnmatched,
      observation_usage_exact: observationUsageExact,
      response_usage_matches_raw: sameUsage(raw.usage, callUsage),
      metric_usage_matches_raw: sameUsage(raw.usage, metricUsage),
    },
  };
}

function metricsHealthCorrelation(raw) {
  const samples = Array.isArray(raw.metrics_health_samples) ? raw.metrics_health_samples : [];
  const initial = samples.find((sample) => sample.stage === "initial") ?? null;
  const final = [...samples].reverse().find(
    (sample) => sample.stage === "final" || sample.stage === "correlation",
  ) ?? null;
  const expectedDelta = Number.isSafeInteger(raw.counters.end_accepted)
    && Number.isSafeInteger(raw.counters.start_accepted)
    ? raw.counters.end_accepted - raw.counters.start_accepted
    : null;
  const continuous = raw.worker_boundary !== null && samples.every((sample) => (
    sample.instance_id === raw.worker_boundary.instance_id
      && sample.worker_source_sha256 === raw.worker_boundary.worker_source_sha256
      && sample.status === "ok"
      && sample.writable_verified === true
      && sample.write_failures_total === 0
      && sample.last_error_code === null
  ));
  const attemptedDelta = initial && final
    ? final.records_attempted - initial.records_attempted
    : null;
  const writtenDelta = initial && final
    ? final.records_written - initial.records_written
    : null;
  return {
    pass: initial !== null
      && final !== null
      && continuous
      && initial.pending_records === 0
      && final.pending_records === 0
      && attemptedDelta === expectedDelta
      && writtenDelta === expectedDelta,
    actual: {
      samples: samples.length,
      expected_delta: expectedDelta,
      attempted_delta: attemptedDelta,
      written_delta: writtenDelta,
      initial_pending: initial?.pending_records ?? null,
      final_pending: final?.pending_records ?? null,
      continuous,
    },
  };
}

function commonGates(plan, raw, summary) {
  const plannedInvocations = planInvocationCount(plan);
  const plannedCalls = planLiveCallCount(plan);
  const observedAttempts = raw.observations.reduce(
    (total, row) => total + row.planned_calls,
    0,
  );
  const acceptedDelta = summary.counters.accepted_delta;
  const settledDelta = summary.counters.settled_delta;
  const metrics = metricCorrelation(raw);
  const metricsHealth = metricsHealthCorrelation(raw);
  const requiresMetrics = plan.execution_mode === "live";
  const sourceBound = !requiresMetrics || (
    typeof raw.source?.worker_source_sha256 === "string"
      && raw.source.worker_source_sha256 === raw.worker_boundary?.worker_source_sha256
      && raw.source_end?.commit === raw.source.commit
      && raw.source_end?.dirty === false
      && raw.source_end?.worker_source_sha256 === raw.source.worker_source_sha256
  );
  const gates = [
    gate("registered_plan_digest", raw.plan_digest === planDigest(plan), raw.plan_digest, planDigest(plan)),
    gate("clean_source", raw.source?.dirty === false, raw.source?.dirty, false),
    gate(
      "worker_source_bound_to_clean_commit",
      sourceBound,
      {
        start_commit: raw.source?.commit ?? null,
        end_commit: raw.source_end?.commit ?? null,
        expected_worker_source_sha256: raw.source?.worker_source_sha256 ?? null,
        actual_worker_source_sha256: raw.worker_boundary?.worker_source_sha256 ?? null,
        end_worker_source_sha256: raw.source_end?.worker_source_sha256 ?? null,
      },
      "same_clean_commit_and_worker_digest",
      requiresMetrics,
      requiresMetrics,
    ),
    gate("run_completed", raw.interrupted === false, raw.stop_reason, null),
    gate("no_automatic_retries", raw.automatic_retries === 0, raw.automatic_retries, 0),
    gate(
      "exact_invocation_count",
      raw.observations.length === plannedInvocations,
      raw.observations.length,
      plannedInvocations,
    ),
    gate(
      "live_call_budget",
      raw.live_call_count_known === true
        && raw.observed_live_calls <= plan.budgets.live_calls,
      raw.observed_live_calls,
      `<=${plan.budgets.live_calls}`,
    ),
    gate(
      "live_call_count_known",
      raw.live_call_count_known === true,
      raw.live_call_count_known,
      true,
    ),
    gate(
      "planned_live_calls_observed",
      plan.execution_mode !== "live" || raw.observed_live_calls === plannedCalls,
      raw.observed_live_calls,
      plan.execution_mode === "live" ? plannedCalls : 0,
    ),
    gate(
      "input_token_budget",
      summary.usage.input_tokens <= plan.budgets.input_tokens,
      summary.usage.input_tokens,
      `<=${plan.budgets.input_tokens}`,
    ),
    gate(
      "output_token_budget",
      summary.usage.output_tokens <= plan.budgets.output_tokens,
      summary.usage.output_tokens,
      `<=${plan.budgets.output_tokens}`,
    ),
    gate("worker_boundary_continuity", healthContinuity(raw), healthContinuity(raw), true),
    gate(
      "exact_accepted_counter_delta",
      acceptedDelta === observedAttempts,
      acceptedDelta,
      observedAttempts,
    ),
    gate(
      "exact_settled_counter_delta",
      settledDelta === observedAttempts,
      settledDelta,
      observedAttempts,
    ),
    gate("final_counters_balanced", summary.counters.final_balanced, summary.counters, "balanced"),
    gate("final_worker_idle", summary.counters.final_active === 0 && summary.counters.final_queued === 0, {
      active: summary.counters.final_active,
      queued: summary.counters.final_queued,
    }, { active: 0, queued: 0 }),
    gate(
      "worker_metric_correlation",
      metrics.pass,
      metrics.actual,
      "schema_v2_exact_correlation",
      requiresMetrics,
      requiresMetrics,
    ),
    gate(
      "metrics_health_continuity",
      metricsHealth.pass,
      metricsHealth.actual,
      "healthy_exact_counter_delta",
      requiresMetrics,
      requiresMetrics,
    ),
  ];
  return gates;
}

function canaryGates(raw, summary) {
  const nonCancelled = raw.observations.filter((row) => row.expected_outcome !== "cancelled");
  const errorCodes = Object.keys(summary.errors.by_code);
  return [
    gate(
      "canary_exact_15_live_calls",
      raw.observed_live_calls === 15,
      raw.observed_live_calls,
      15,
    ),
    gate(
      "all_non_cancelled_requests_correct",
      nonCancelled.length === 14 && nonCancelled.every((row) => row.correct),
      nonCancelled.filter((row) => row.correct).length,
      14,
    ),
    gate(
      "forbidden_live_errors_zero",
      errorCodes.every((code) => !FORBIDDEN_LIVE_ERRORS.has(code)),
      errorCodes.filter((code) => FORBIDDEN_LIVE_ERRORS.has(code)),
      [],
    ),
    gate(
      "serial_maximum_latency",
      summary.latency.serial.maximum_ms !== null
        && summary.latency.serial.maximum_ms <= 25_000,
      summary.latency.serial.maximum_ms,
      "<=25000ms",
    ),
    gate(
      "parallel_two_p95_latency",
      summary.latency.parallel_two.p95_ms !== null
        && summary.latency.parallel_two.p95_ms <= 25_000,
      summary.latency.parallel_two.p95_ms,
      "<=25000ms",
    ),
    gate(
      "overall_maximum_latency",
      summary.latency.successful_requests.maximum_ms !== null
        && summary.latency.successful_requests.maximum_ms <= 45_000,
      summary.latency.successful_requests.maximum_ms,
      "<=45000ms",
    ),
    gate(
      "concurrency_two_throughput_gain",
      summary.throughput.parallel_to_serial_ratio !== null
        && summary.throughput.parallel_to_serial_ratio >= 1.5,
      summary.throughput.parallel_to_serial_ratio,
      ">=1.5",
    ),
    gate(
      "cancellation_recovery",
      summary.recovery.cancellation_observations === 1
        && summary.recovery.cancellation_recovered
        && summary.recovery.cancellation_recovery_ms.count === 1
        && summary.recovery.cancellation_recovery_ms.maximum_ms !== null
        && summary.recovery.cancellation_recovery_ms.maximum_ms <= 5_000,
      {
        observations: summary.recovery.cancellation_observations,
        recovered: summary.recovery.cancellation_recovered,
        maximum_ms: summary.recovery.cancellation_recovery_ms.maximum_ms,
      },
      { observations: 1, recovered: true, maximum_ms: "<=5000" },
    ),
  ];
}

function scenarioGates(plan, raw) {
  return plan.required_scenarios.map((id) => gate(
    `scenario_${id}`,
    scenarioStatus(raw, id) === "pass",
    scenarioStatus(raw, id),
    "pass",
  ));
}

function commercialGates(plan, raw, summary) {
  if (plan.id !== "commercial_candidate") {
    return [];
  }
  const byPhase = Object.fromEntries(plan.phases.map((phase) => [
    phase.id,
    raw.observations.filter((row) => row.phase_id === phase.id && row.correct).length,
  ]));
  const product = summary.product_quality;
  return [
    gate(
      "certification_provenance_bound",
      false,
      "not_implemented",
      "trusted_git_product_scenario_resource_connectors",
    ),
    ...Object.entries(byPhase).map(([phase, count]) => gate(
      `minimum_samples_${phase}`,
      count >= 30,
      count,
      ">=30",
    )),
    gate(
      "both_product_workloads_present",
      (summary.workloads.starring_v15_one_call ?? 0) >= 30
        && (summary.workloads.starring_v15_two_call ?? 0) >= 30,
      summary.workloads,
      { starring_v15_one_call: ">=30", starring_v15_two_call: ">=30" },
    ),
    gate(
      "product_quality_exact",
      product.observations > 0
        && product.exact_semantics === product.observations
        && product.validation_current === product.observations
        && product.simulation_current === product.observations
        && product.candidate_only === product.observations,
      product,
      "100%",
    ),
    gate(
      "six_hour_resource_telemetry",
      summary.resources.duration_ms >= 6 * 60 * 60_000
        && summary.resources.sample_count > 0
        && summary.resources.error_count === 0,
      summary.resources,
      { duration_ms: ">=21600000", samples: ">0", errors: 0 },
    ),
  ];
}

export function assessRun(planInput, raw, summary) {
  const plan = assertKnownPlan(structuredClone(planInput));
  if (canonicalJson(raw.plan) !== canonicalJson(plan) || summary.plan_id !== plan.id) {
    throw new Error("plan_run_mismatch");
  }
  const recomputedSummary = summarizeRun(raw);
  if (canonicalJson(summary) !== canonicalJson(recomputedSummary)) {
    throw new Error("summary_run_mismatch");
  }
  summary = recomputedSummary;
  const common = commonGates(plan, raw, summary);
  const planSpecific = plan.id === "live_canary" ? canaryGates(raw, summary) : [];
  const scenarios = scenarioGates(plan, raw);
  const commercial = commercialGates(plan, raw, summary);
  const gates = [...common, ...planSpecific, ...scenarios, ...commercial];
  const diagnosticComplete = allPass([...common, ...planSpecific, ...scenarios]);
  const commercialCertified = plan.id === "commercial_candidate" && allPass(gates);
  const availabilitySupported = commercialCertified && summary.resources.duration_ms >= 24 * 60 * 60_000;
  const nonClaims = [];
  if (plan.id !== "commercial_candidate") {
    nonClaims.push("commercial_slo_not_certified");
  }
  if (!commercialCertified) {
    nonClaims.push("production_capacity_not_certified");
  }
  if (!availabilitySupported) {
    nonClaims.push("annual_availability_not_inferred");
  }
  if (plan.execution_mode === "fake_only") {
    nonClaims.push("no_live_luna_performance_claim");
  }
  if (plan.phases.some((phase) => phase.expected_outcome === "cancelled")) {
    nonClaims.push("cancelled_request_token_usage_unobserved");
  }
  if (plan.id === "commercial_candidate") {
    nonClaims.push("certification_connectors_not_bound");
  }
  if (raw.evidence_completeness !== undefined
    && raw.evidence_completeness !== "complete") {
    nonClaims.push("execution_evidence_incomplete");
  }
  if (raw.live_call_count_known !== true) {
    nonClaims.push("live_call_and_usage_observation_incomplete");
  }
  return {
    schema_version: 1,
    run_id: raw.run_id,
    plan_id: plan.id,
    claim_scope: plan.claim_scope,
    verdict: allPass(gates) ? "pass" : "fail",
    gates,
    claims: {
      diagnostic_complete: diagnosticComplete,
      eligible_for_step_load: plan.id === "live_canary" && diagnosticComplete,
      commercial_slo_certified: commercialCertified,
      availability_24h_supported: availabilitySupported,
    },
    non_claims: [...new Set(nonClaims)].sort(),
  };
}

export { METRIC_REQUIRED_FIELDS };
