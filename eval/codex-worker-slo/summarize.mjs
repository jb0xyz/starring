export class SummaryError extends Error {
  constructor(code) {
    super(code);
    this.name = "SummaryError";
    this.code = code;
  }
}

function finite(values) {
  return values.filter((value) => Number.isFinite(value) && value >= 0);
}

export function nearestRank(values, fraction) {
  if (!Number.isFinite(fraction) || fraction <= 0 || fraction > 1) {
    throw new SummaryError("invalid_percentile");
  }
  const sorted = finite(values).sort((left, right) => left - right);
  if (sorted.length === 0) {
    return null;
  }
  return sorted[Math.ceil(sorted.length * fraction) - 1];
}

function mean(values) {
  const usable = finite(values);
  if (usable.length === 0) {
    return null;
  }
  return usable.reduce((sum, value) => sum + value, 0) / usable.length;
}

function distribution(values) {
  const usable = finite(values);
  return {
    count: usable.length,
    minimum_ms: usable.length === 0 ? null : Math.min(...usable),
    mean_ms: mean(usable),
    p50_ms: nearestRank(usable, 0.5),
    p95_ms: nearestRank(usable, 0.95),
    p99_ms: nearestRank(usable, 0.99),
    maximum_ms: usable.length === 0 ? null : Math.max(...usable),
  };
}

function countsBy(values) {
  return Object.fromEntries([...values.reduce((counts, value) => {
    counts.set(value, (counts.get(value) ?? 0) + 1);
    return counts;
  }, new Map())].sort(([left], [right]) => left.localeCompare(right)));
}

function sum(values) {
  return finite(values).reduce((total, value) => total + value, 0);
}

function invocationLatencies(observation) {
  if (!observation.correct || observation.warmup) {
    return [];
  }
  return [observation.latency_ms];
}

function modelCallLatencies(observation) {
  if (!observation.correct || observation.warmup) {
    return [];
  }
  return observation.calls.map((call) => call.latency_ms);
}

function invocationThroughput(raw, phaseId) {
  const waves = raw.waves.filter((wave) => wave.phase_id === phaseId);
  const invocations = raw.observations
    .filter((row) => row.phase_id === phaseId && row.correct).length;
  const durationMs = sum(waves.map((wave) => wave.duration_ms));
  return {
    invocations,
    duration_ms: durationMs,
    invocations_per_second: durationMs > 0
      ? invocations / (durationMs / 1_000)
      : null,
  };
}

function modelCallThroughput(raw, phaseId) {
  const waves = raw.waves.filter((wave) => wave.phase_id === phaseId);
  const modelCalls = raw.observations
    .filter((row) => row.phase_id === phaseId && row.correct)
    .reduce((total, row) => total + row.calls.length, 0);
  const durationMs = sum(waves.map((wave) => wave.duration_ms));
  return {
    model_calls: modelCalls,
    duration_ms: durationMs,
    model_calls_per_second: durationMs > 0
      ? modelCalls / (durationMs / 1_000)
      : null,
  };
}

function throughputRatio(serial, parallel) {
  return serial > 0 && parallel !== null ? parallel / serial : null;
}

function compatibleInvocationThroughput(value) {
  return {
    calls: value.invocations,
    duration_ms: value.duration_ms,
    calls_per_second: value.invocations_per_second,
  };
}

function observationRequestIds(observation) {
  const callIds = observation.calls
    .map((call) => call.request_id)
    .filter((requestId) => typeof requestId === "string" && requestId.length > 0);
  if (callIds.length > 0) {
    return callIds;
  }
  return typeof observation.request_id === "string" && observation.request_id.length > 0
    ? [observation.request_id]
    : [];
}

function resourceRange(samples, key) {
  const values = finite(samples.map((sample) => sample[key]));
  return {
    minimum: values.length === 0 ? null : Math.min(...values),
    maximum: values.length === 0 ? null : Math.max(...values),
    mean: mean(values),
  };
}

function counterDelta(end, start) {
  return Number.isSafeInteger(end) && Number.isSafeInteger(start) ? end - start : null;
}

export function summarizeRun(raw) {
  if (!raw || raw.schema_version !== 1 || !Array.isArray(raw.observations)
    || !Array.isArray(raw.waves) || !Array.isArray(raw.health_samples)) {
    throw new SummaryError("invalid_raw_run");
  }
  const completed = raw.observations.filter((row) => row.outcome === "completed");
  const correct = raw.observations.filter((row) => row.correct);
  const expectedCancelled = raw.observations.filter(
    (row) => row.expected_outcome === "cancelled" && row.outcome === "cancelled",
  );
  const unexpected = raw.observations.filter((row) => !row.expected_outcome_met);
  const plannedAttempts = sum(raw.observations.map((row) => row.planned_calls));
  const correctAttempts = sum(correct.map((row) => row.calls.length));
  const unexpectedAttempts = sum(unexpected.map((row) => row.planned_calls));
  const successfulInvocationLatency = raw.observations.flatMap(invocationLatencies);
  const successfulModelCallLatency = raw.observations.flatMap(modelCallLatencies);
  const serialInvocationLatency = raw.observations
    .filter((row) => row.phase_id === "serial")
    .flatMap(invocationLatencies);
  const parallelInvocationLatency = raw.observations
    .filter((row) => row.phase_id === "parallel_two")
    .flatMap(invocationLatencies);
  const serialModelCallLatency = raw.observations
    .filter((row) => row.phase_id === "serial")
    .flatMap(modelCallLatencies);
  const parallelModelCallLatency = raw.observations
    .filter((row) => row.phase_id === "parallel_two")
    .flatMap(modelCallLatencies);
  const serialInvocationThroughput = invocationThroughput(raw, "serial");
  const parallelInvocationThroughput = invocationThroughput(raw, "parallel_two");
  const serialModelCallThroughput = modelCallThroughput(raw, "serial");
  const parallelModelCallThroughput = modelCallThroughput(raw, "parallel_two");
  const invocationThroughputRatio = throughputRatio(
    serialInvocationThroughput.invocations_per_second,
    parallelInvocationThroughput.invocations_per_second,
  );
  const modelCallThroughputRatio = throughputRatio(
    serialModelCallThroughput.model_calls_per_second,
    parallelModelCallThroughput.model_calls_per_second,
  );
  const health = raw.health_samples;
  const workerMetrics = Array.isArray(raw.worker_metrics) ? raw.worker_metrics : [];
  const queueWait = workerMetrics.map((metric) => metric.queue_wait_ms);
  const runnerDuration = workerMetrics.map((metric) => metric.runner_duration_ms);
  const workerTotal = workerMetrics.map((metric) => metric.total_duration_ms);
  const usage = raw.usage ?? {};
  const correctInvocationDivisor = correct.length === 0 ? null : correct.length;
  const correctModelCallDivisor = correctAttempts === 0 ? null : correctAttempts;
  const cancellationRecovery = expectedCancelled
    .map((row) => row.cancellation_recovery_ms)
    .filter(Number.isFinite);
  const productRows = raw.observations.filter((row) => row.product !== null);
  const errorCodes = unexpected.map((row) => row.error_code ?? "unexpected_outcome");
  if (raw.stop_reason && unexpected.length === 0) {
    errorCodes.push(raw.stop_reason);
  }
  return {
    schema_version: 1,
    run_id: raw.run_id,
    plan_id: raw.plan.id,
    plan_revision: raw.plan.revision,
    claim_scope: raw.plan.claim_scope,
    execution_mode: raw.execution_mode,
    interrupted: raw.interrupted,
    stop_reason: raw.stop_reason,
    duration_ms: raw.duration_ms,
    counts: {
      planned_invocations: raw.plan.phases.reduce(
        (total, phase) => total + phase.concurrency * phase.waves,
        0,
      ),
      observed_invocations: raw.observations.length,
      planned_attempts: plannedAttempts,
      observed_live_calls: raw.observed_live_calls,
      live_call_count_known: raw.live_call_count_known,
      completed_invocations: completed.length,
      correct_invocations: correct.length,
      correct_attempts: correctAttempts,
      expected_cancellations: expectedCancelled.length,
      unexpected_invocations: unexpected.length,
      unexpected_attempts: unexpectedAttempts,
      automatic_retries: raw.automatic_retries,
      worker_metrics: workerMetrics.length,
      metrics_health_samples: raw.metrics_health_samples?.length ?? 0,
      resource_samples: raw.resource_samples.length,
      health_samples: raw.health_samples.length,
    },
    rates: {
      correct_completion: plannedAttempts === 0 ? null : correctAttempts / plannedAttempts,
      unexpected_error: plannedAttempts === 0 ? null : unexpectedAttempts / plannedAttempts,
    },
    latency: {
      end_to_end_invocations: {
        successful: distribution(successfulInvocationLatency),
        serial: distribution(serialInvocationLatency),
        parallel_two: distribution(parallelInvocationLatency),
      },
      model_calls: {
        successful: distribution(successfulModelCallLatency),
        serial: distribution(serialModelCallLatency),
        parallel_two: distribution(parallelModelCallLatency),
      },
      successful_requests: distribution(successfulInvocationLatency),
      serial: distribution(serialInvocationLatency),
      parallel_two: distribution(parallelInvocationLatency),
    },
    worker_timing: {
      expected_records: raw.observations.reduce(
        (total, row) => total + observationRequestIds(row).length,
        0,
      ),
      observed_records: workerMetrics.length,
      queue_wait: distribution(queueWait),
      runner_duration: distribution(runnerDuration),
      total_duration: distribution(workerTotal),
    },
    throughput: {
      end_to_end_invocations: {
        serial: serialInvocationThroughput,
        parallel_two: parallelInvocationThroughput,
        parallel_to_serial_ratio: invocationThroughputRatio,
      },
      model_calls: {
        serial: serialModelCallThroughput,
        parallel_two: parallelModelCallThroughput,
        parallel_to_serial_ratio: modelCallThroughputRatio,
      },
      serial: compatibleInvocationThroughput(serialInvocationThroughput),
      parallel_two: compatibleInvocationThroughput(parallelInvocationThroughput),
      parallel_to_serial_ratio: invocationThroughputRatio,
    },
    saturation: {
      maximum_active: health.length === 0
        ? null
        : Math.max(...health.map((sample) => sample.active_requests)),
      maximum_queued: health.length === 0
        ? null
        : Math.max(...health.map((sample) => sample.queued_requests)),
    },
    usage: {
      input_tokens: usage.input_tokens ?? 0,
      cached_input_tokens: usage.cached_input_tokens ?? 0,
      output_tokens: usage.output_tokens ?? 0,
      reasoning_output_tokens: usage.reasoning_output_tokens ?? 0,
      mean_input_tokens_per_correct_invocation: correctInvocationDivisor === null
        ? null
        : (usage.input_tokens ?? 0) / correctInvocationDivisor,
      mean_output_tokens_per_correct_invocation: correctInvocationDivisor === null
        ? null
        : (usage.output_tokens ?? 0) / correctInvocationDivisor,
      mean_input_tokens_per_correct_model_call: correctModelCallDivisor === null
        ? null
        : (usage.input_tokens ?? 0) / correctModelCallDivisor,
      mean_output_tokens_per_correct_model_call: correctModelCallDivisor === null
        ? null
        : (usage.output_tokens ?? 0) / correctModelCallDivisor,
      mean_input_tokens_per_correct_request: correctModelCallDivisor === null
        ? null
        : (usage.input_tokens ?? 0) / correctModelCallDivisor,
      mean_output_tokens_per_correct_request: correctModelCallDivisor === null
        ? null
        : (usage.output_tokens ?? 0) / correctModelCallDivisor,
    },
    recovery: {
      cancellation_observations: expectedCancelled.length,
      cancellation_recovered: expectedCancelled.length > 0
        && expectedCancelled.every((row) => row.cancellation_recovered === true),
      cancellation_recovery_ms: distribution(cancellationRecovery),
      restart_recovery_ms: distribution(raw.restart_recovery_ms ?? []),
    },
    resources: {
      duration_ms: raw.resource_duration_ms,
      sample_count: raw.resource_samples.length,
      error_count: raw.resource_errors.length,
      rss_bytes: resourceRange(raw.resource_samples, "rss_bytes"),
      heap_used_bytes: resourceRange(raw.resource_samples, "heap_used_bytes"),
      cpu_percent: resourceRange(raw.resource_samples, "cpu_percent"),
      evaluator_event_loop_delay_p99_ms: resourceRange(
        raw.resource_samples,
        "evaluator_event_loop_delay_p99_ms",
      ),
    },
    counters: {
      accepted_delta: counterDelta(raw.counters.end_accepted, raw.counters.start_accepted),
      settled_delta: counterDelta(raw.counters.end_settled, raw.counters.start_settled),
      final_balanced: raw.counters.end_accepted === raw.counters.end_settled,
      final_active: health.at(-1)?.active_requests ?? null,
      final_queued: health.at(-1)?.queued_requests ?? null,
    },
    workloads: countsBy(raw.observations.map((row) => row.workload_id)),
    product_quality: {
      observations: productRows.length,
      exact_semantics: productRows.filter((row) => row.product.exact_semantics).length,
      validation_current: productRows.filter((row) => row.product.validation_current).length,
      simulation_current: productRows.filter((row) => row.product.simulation_current).length,
      candidate_only: productRows.filter((row) => row.product.candidate_only).length,
    },
    errors: {
      count: errorCodes.length,
      by_code: countsBy(errorCodes),
    },
  };
}
