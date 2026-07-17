import { randomUUID } from "node:crypto";
import { performance } from "node:perf_hooks";
import {
  assertKnownPlan,
  assertPlanBudget,
  planDigest,
  planLiveCallCount,
} from "./plans.mjs";
import {
  WorkloadError,
  buildTransportRequest,
  getWorkload,
  validateProductResult,
  validateTransportCompletion,
} from "./workloads.mjs";

export class SloRunError extends Error {
  constructor(code) {
    super(code);
    this.name = "SloRunError";
    this.code = code;
  }
}

function finiteDuration(value) {
  return Number.isFinite(value) && value >= 0 ? Math.round(value) : 0;
}

function safeErrorCode(value, fallback = "request_failed") {
  return typeof value === "string" && /^[a-z][a-z0-9_]{0,127}$/.test(value)
    ? value
    : fallback;
}

function addUsage(left, right) {
  return {
    input_tokens: left.input_tokens + right.input_tokens,
    cached_input_tokens: left.cached_input_tokens + right.cached_input_tokens,
    output_tokens: left.output_tokens + right.output_tokens,
    reasoning_output_tokens: left.reasoning_output_tokens + right.reasoning_output_tokens,
  };
}

function zeroUsage() {
  return {
    input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
  };
}

function usageFromCalls(calls) {
  return calls.reduce((total, call) => addUsage(total, call.usage), zeroUsage());
}

function sanitizeWorkerMetric(metric) {
  const fields = [
    "metric_schema_version",
    "timestamp",
    "request_id",
    "provider",
    "model",
    "reasoning_effort",
    "frontier_name",
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
  if (!metric || typeof metric !== "object" || Array.isArray(metric)
    || !fields.every((key) => Object.hasOwn(metric, key))) {
    throw new SloRunError("invalid_worker_metric");
  }
  return Object.fromEntries(fields.map((key) => [key, structuredClone(metric[key])]));
}

function sanitizeScenarios(value) {
  if (value === undefined) {
    return [];
  }
  if (!Array.isArray(value)) {
    throw new SloRunError("invalid_scenario_results");
  }
  const seen = new Set();
  return value.map((entry) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)
      || typeof entry.id !== "string"
      || !/^[a-z][a-z0-9_]{0,127}$/.test(entry.id)
      || !["pass", "fail", "not_evaluated"].includes(entry.status)
      || !Number.isFinite(entry.duration_ms)
      || entry.duration_ms < 0
      || seen.has(entry.id)) {
      throw new SloRunError("invalid_scenario_results");
    }
    seen.add(entry.id);
    return {
      id: entry.id,
      status: entry.status,
      duration_ms: Math.round(entry.duration_ms),
      error_code: entry.error_code === null
        ? null
        : safeErrorCode(entry.error_code, "scenario_failed"),
    };
  });
}

function exactProfile(health, plan) {
  return health.concurrency_limit === plan.worker_profile.concurrency_limit
    && health.queue_capacity === plan.worker_profile.queue_capacity
    && health.request_timeout_ms === plan.worker_profile.request_timeout_ms;
}

function exactIdentity(health, plan) {
  return Object.entries(plan.identity).every(([key, value]) => health[key] === value);
}

export function validateWorkerHealth(health, plan, boundary = null) {
  if (!health || typeof health !== "object" || Array.isArray(health)
    || health.schema_version !== 1
    || health.status !== "ok"
    || !exactIdentity(health, plan)
    || !exactProfile(health, plan)
    || typeof health.instance_id !== "string"
    || health.instance_id.length === 0
    || typeof health.worker_source_sha256 !== "string"
    || !/^[0-9a-f]{64}$/.test(health.worker_source_sha256)) {
    throw new SloRunError("worker_boundary_mismatch");
  }
  const counters = [
    "active_requests",
    "queued_requests",
    "accepted_requests_total",
    "settled_requests_total",
  ];
  if (!counters.every((key) => Number.isSafeInteger(health[key]) && health[key] >= 0)
    || health.settled_requests_total > health.accepted_requests_total
    || health.accepted_requests_total - health.settled_requests_total
      !== health.active_requests + health.queued_requests) {
    throw new SloRunError("worker_counter_invalid");
  }
  if (boundary
    && (health.instance_id !== boundary.instance_id
      || health.worker_source_sha256 !== boundary.worker_source_sha256)) {
    throw new SloRunError("worker_continuity_lost");
  }
  return structuredClone(health);
}

export function validateMetricsHealth(health, boundary = null) {
  const keys = [
    "schema_version",
    "instance_id",
    "worker_source_sha256",
    "status",
    "writable_verified",
    "records_attempted",
    "records_written",
    "pending_records",
    "write_failures_total",
    "last_error_code",
  ];
  if (!health || typeof health !== "object" || Array.isArray(health)
    || Object.keys(health).sort().join("\u0000") !== keys.sort().join("\u0000")
    || health.schema_version !== 1
    || !["ok", "degraded"].includes(health.status)
    || typeof health.writable_verified !== "boolean"
    || typeof health.instance_id !== "string"
    || typeof health.worker_source_sha256 !== "string"
    || !/^[0-9a-f]{64}$/.test(health.worker_source_sha256)
    || ![
      "records_attempted",
      "records_written",
      "pending_records",
      "write_failures_total",
    ].every((key) => Number.isSafeInteger(health[key]) && health[key] >= 0)
    || health.records_written > health.records_attempted
    || health.pending_records > health.records_attempted
    || health.records_written + health.pending_records + health.write_failures_total
      !== health.records_attempted
    || (health.last_error_code !== null && health.last_error_code !== "metrics_write_failed")) {
    throw new SloRunError("metrics_health_invalid");
  }
  if (boundary
    && (health.instance_id !== boundary.instance_id
      || health.worker_source_sha256 !== boundary.worker_source_sha256)) {
    throw new SloRunError("metrics_health_continuity_lost");
  }
  return structuredClone(health);
}

function validateSource(source, requireWorkerDigest) {
  if (!source || typeof source !== "object" || Array.isArray(source)
    || typeof source.commit !== "string"
    || !/^[0-9a-f]{40}$/.test(source.commit)
    || typeof source.dirty !== "boolean"
    || (requireWorkerDigest
      && (typeof source.worker_source_sha256 !== "string"
        || !/^[0-9a-f]{64}$/.test(source.worker_source_sha256)))) {
    throw new SloRunError("invalid_source_boundary");
  }
  if (source.dirty) {
    throw new SloRunError("dirty_source_forbidden");
  }
  return structuredClone(source);
}

function validateRunId(runId) {
  if (typeof runId !== "string" || !/^[a-z0-9][a-z0-9_-]{0,127}$/.test(runId)) {
    throw new SloRunError("invalid_run_id");
  }
  return runId;
}

function validateLoopbackBaseUrl(value) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new SloRunError("invalid_worker_url");
  }
  if (parsed.protocol !== "http:"
    || parsed.hostname !== "127.0.0.1"
    || (parsed.pathname !== "/" && parsed.pathname !== "")
    || parsed.username
    || parsed.password
    || parsed.search
    || parsed.hash) {
    throw new SloRunError("loopback_worker_required");
  }
  return parsed.origin;
}

function abortedRunError(signal, fallback = "duration_budget_exceeded") {
  return new SloRunError(safeErrorCode(signal?.reason?.code, fallback));
}

function runWithSignal(signal, operation, fallback = "duration_budget_exceeded") {
  if (signal?.aborted) {
    return Promise.reject(abortedRunError(signal, fallback));
  }
  if (!signal) {
    return Promise.resolve().then(operation);
  }
  return new Promise((resolvePromise, rejectPromise) => {
    const abort = () => rejectPromise(abortedRunError(signal, fallback));
    signal.addEventListener("abort", abort, { once: true });
    Promise.resolve()
      .then(operation)
      .then(
        (value) => {
          signal.removeEventListener("abort", abort);
          resolvePromise(value);
        },
        (error) => {
          signal.removeEventListener("abort", abort);
          rejectPromise(error);
        },
      );
  });
}

function linkedAbortController(parentSignal) {
  const controller = new AbortController();
  const abort = () => controller.abort(abortedRunError(parentSignal));
  if (parentSignal.aborted) {
    abort();
  } else {
    parentSignal.addEventListener("abort", abort, { once: true });
  }
  return {
    controller,
    unlink: () => parentSignal.removeEventListener("abort", abort),
  };
}

function scheduleRunDeadline(controller, milliseconds) {
  const timer = setTimeout(
    () => controller.abort(new SloRunError("duration_budget_exceeded")),
    milliseconds,
  );
  return () => clearTimeout(timer);
}

async function responseJson(response, signal) {
  const text = await runWithSignal(signal, () => response.text());
  if (Buffer.byteLength(text) > 1_000_000) {
    throw new SloRunError("worker_response_too_large");
  }
  try {
    return JSON.parse(text);
  } catch {
    throw new SloRunError("worker_response_invalid_json");
  }
}

function createLiveHealthReader(baseUrl, token, fetchFn) {
  return async (signal) => {
    let response;
    try {
      response = await runWithSignal(signal, () => fetchFn(`${baseUrl}/health`, {
        headers: { authorization: `Bearer ${token}` },
        signal,
      }));
    } catch {
      if (signal?.aborted) {
        throw abortedRunError(signal);
      }
      throw new SloRunError("worker_health_unreachable");
    }
    if (response.status !== 200) {
      throw new SloRunError("worker_health_failed");
    }
    return responseJson(response, signal);
  };
}

function createLiveMetricsHealthReader(baseUrl, token, fetchFn) {
  return async (signal) => {
    let response;
    try {
      response = await runWithSignal(signal, () => fetchFn(`${baseUrl}/metrics-health`, {
        headers: { authorization: `Bearer ${token}` },
        signal,
      }));
    } catch {
      if (signal?.aborted) {
        throw abortedRunError(signal);
      }
      throw new SloRunError("metrics_health_unreachable");
    }
    if (response.status !== 200) {
      throw new SloRunError("metrics_health_failed");
    }
    return responseJson(response, signal);
  };
}

function validateAdmissionResponse(value, observationId) {
  const keys = ["observation_id", "request_id", "schema_version", "status"];
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).sort().join("\u0000") !== keys.join("\u0000")
    || value.schema_version !== 1
    || value.observation_id !== observationId
    || (value.status !== "active" && value.status !== "queued")
    || typeof value.request_id !== "string"
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value.request_id,
    )) {
    throw new SloRunError("admission_response_invalid");
  }
  return structuredClone(value);
}

function createLiveAdmissionReader(baseUrl, token, fetchFn) {
  return async (observationId, signal) => {
    const url = new URL("/request-admission", baseUrl);
    url.searchParams.set("observation_id", observationId);
    let response;
    try {
      response = await runWithSignal(signal, () => fetchFn(url, {
        headers: { authorization: `Bearer ${token}` },
        signal,
      }));
    } catch {
      if (signal?.aborted) {
        throw abortedRunError(signal);
      }
      throw new SloRunError("admission_unreachable");
    }
    const body = await responseJson(response, signal);
    if (response.status === 404
      && body
      && typeof body === "object"
      && !Array.isArray(body)
      && Object.keys(body).length === 1
      && body.error
      && typeof body.error === "object"
      && !Array.isArray(body.error)
      && Object.keys(body.error).length === 1
      && body.error.code === "admission_not_found") {
      return null;
    }
    if (response.status !== 200) {
      throw new SloRunError("admission_lookup_failed");
    }
    return validateAdmissionResponse(body, observationId);
  };
}

function createFakeBoundary(plan) {
  let accepted = 0;
  let settled = 0;
  const health = () => ({
    schema_version: 1,
    status: "ok",
    ...plan.identity,
    instance_id: "fake-worker-instance",
    worker_source_sha256: "0".repeat(64),
    ...plan.worker_profile,
    active_requests: 0,
    queued_requests: 0,
    accepted_requests_total: accepted,
    settled_requests_total: settled,
  });
  return {
    health,
    settle(count) {
      accepted += count;
      settled += count;
    },
  };
}

function fakeCompleted(sequence, plan) {
  const requestId = `fake-${sequence}`;
  return {
    outcome: "completed",
    correct: true,
    expected_outcome_met: true,
    status_code: 200,
    error_code: null,
    request_id: requestId,
    request_ids: [requestId],
    provider: plan.identity.provider,
    model: plan.identity.model,
    reasoning_effort: plan.identity.reasoning_effort,
    auth_mode: plan.identity.auth_mode,
    codex_cli_version: plan.identity.codex_cli_version,
    frontier_name: "record_slo_probe",
    worker_duration_ms: 0,
    calls: [{
      request_id: requestId,
      status_code: 200,
      latency_ms: 0,
      provider: plan.identity.provider,
      model: plan.identity.model,
      reasoning_effort: plan.identity.reasoning_effort,
      auth_mode: plan.identity.auth_mode,
      codex_cli_version: plan.identity.codex_cli_version,
      frontier_name: "record_slo_probe",
      usage: zeroUsage(),
    }],
    usage: zeroUsage(),
    product: null,
  };
}

function fakeCancelled(requestId = null) {
  return {
    outcome: "cancelled",
    correct: false,
    expected_outcome_met: true,
    status_code: 499,
    error_code: "client_cancelled",
    request_id: requestId,
    request_ids: requestId === null ? [] : [requestId],
    provider: null,
    model: null,
    reasoning_effort: null,
    auth_mode: null,
    codex_cli_version: null,
    frontier_name: "record_slo_probe",
    worker_duration_ms: null,
    calls: [],
    usage: zeroUsage(),
    product: null,
  };
}

function createLiveTransportExecutor(
  baseUrl,
  token,
  fetchFn,
  requestTimeoutMs,
  cancellationAdmissionTimeoutMs,
  healthPollMs,
  clock,
  runSignal,
) {
  const readAdmission = createLiveAdmissionReader(baseUrl, token, fetchFn);
  return async (workload, sequence, context = {}) => {
    const linked = linkedAbortController(runSignal);
    const { controller } = linked;
    const cancellation = workload.executor === "worker_http_cancel";
    const timeoutCode = cancellation ? "cancellation_not_admitted" : "client_timeout";
    const timeoutMs = cancellation
      ? cancellationAdmissionTimeoutMs
      : requestTimeoutMs + 5_000;
    const timer = setTimeout(
      () => controller.abort(new SloRunError(timeoutCode)),
      timeoutMs,
    );
    let admitted = false;
    let admittedRequestId = null;
    try {
      const snapshot = context.admissionSnapshot;
      if (cancellation
        && (!snapshot
          || snapshot.active_requests !== 0
          || snapshot.queued_requests !== 0
          || snapshot.accepted_requests_total !== snapshot.settled_requests_total)) {
        throw new SloRunError("cancellation_admission_boundary_invalid");
      }
      const observationId = cancellation ? randomUUID() : null;
      const requestState = {
        settled: false,
        response: null,
        error: null,
      };
      const headers = {
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
      };
      if (observationId !== null) {
        headers["x-starring-observation-id"] = observationId;
      }
      const requestPromise = runWithSignal(controller.signal, () => fetchFn(
        `${baseUrl}/v1/frontier-completions`,
        {
          method: "POST",
          headers,
          body: JSON.stringify(buildTransportRequest(sequence)),
          signal: controller.signal,
        },
      ), timeoutCode).then(
        (response) => {
          requestState.settled = true;
          requestState.response = response;
        },
        (error) => {
          requestState.settled = true;
          requestState.error = error;
        },
      );
      if (cancellation) {
        while (!requestState.settled && !controller.signal.aborted) {
          const admission = await readAdmission(observationId, controller.signal);
          if (admission !== null && !requestState.settled) {
            admittedRequestId = admission.request_id;
            if (admission.status !== "active") {
              controller.abort(new SloRunError("cancellation_not_active"));
              break;
            }
            admitted = true;
            clearTimeout(timer);
            controller.abort(new SloRunError("client_cancelled"));
            break;
          }
          await runWithSignal(
            controller.signal,
            () => clock.sleep(healthPollMs),
            timeoutCode,
          );
        }
        if (!admitted && !requestState.settled && !controller.signal.aborted) {
          controller.abort(new SloRunError("cancellation_not_admitted"));
        }
      }
      await requestPromise;
      if (requestState.error) {
        throw requestState.error;
      }
      const response = requestState.response;
      const body = await responseJson(response, controller.signal);
      if (cancellation) {
        if (response.status !== 200) {
          return {
            outcome: "failed",
            correct: false,
            expected_outcome_met: false,
            status_code: response.status,
            error_code: safeErrorCode(body?.error?.code, "worker_http_error"),
            request_id: null,
            request_ids: [],
            provider: null,
            model: null,
            reasoning_effort: null,
            auth_mode: null,
            codex_cli_version: null,
            frontier_name: workload.expected_frontier,
            worker_duration_ms: null,
            calls: [],
            usage: zeroUsage(),
            product: null,
          };
        }
        const valid = validateTransportCompletion(response.status, body, sequence);
        return {
          outcome: "completed",
          correct: false,
          expected_outcome_met: false,
          status_code: 200,
          error_code: "cancellation_not_observed",
          ...valid,
          request_ids: [valid.request_id],
          calls: [{
            request_id: valid.request_id,
            status_code: 200,
            latency_ms: valid.worker_duration_ms,
            provider: valid.provider,
            model: valid.model,
            reasoning_effort: valid.reasoning_effort,
            auth_mode: valid.auth_mode,
            codex_cli_version: valid.codex_cli_version,
            frontier_name: valid.frontier_name,
            usage: valid.usage,
          }],
          product: null,
        };
      }
      if (response.status !== 200) {
        return {
          outcome: "failed",
          correct: false,
          expected_outcome_met: false,
          status_code: response.status,
          error_code: safeErrorCode(body?.error?.code, "worker_http_error"),
          request_id: null,
          request_ids: [],
          provider: null,
          model: null,
          reasoning_effort: null,
          auth_mode: null,
          codex_cli_version: null,
          frontier_name: workload.expected_frontier,
          worker_duration_ms: null,
          calls: [],
          usage: zeroUsage(),
          product: null,
        };
      }
      const valid = validateTransportCompletion(response.status, body, sequence);
      return {
        outcome: "completed",
        correct: true,
        expected_outcome_met: true,
        status_code: 200,
        error_code: null,
        ...valid,
        request_ids: [valid.request_id],
        calls: [{
          request_id: valid.request_id,
          status_code: 200,
          latency_ms: valid.worker_duration_ms,
          provider: valid.provider,
          model: valid.model,
          reasoning_effort: valid.reasoning_effort,
          auth_mode: valid.auth_mode,
          codex_cli_version: valid.codex_cli_version,
          frontier_name: valid.frontier_name,
          usage: valid.usage,
        }],
        product: null,
      };
    } catch (error) {
      if (cancellation && !controller.signal.aborted) {
        controller.abort(new SloRunError(safeErrorCode(error?.code, "admission_lookup_failed")));
      }
      const errorCode = controller.signal.aborted
        ? safeErrorCode(controller.signal.reason?.code, timeoutCode)
        : safeErrorCode(error?.code, error instanceof WorkloadError ? error.code : "transport_failed");
      if (cancellation && admitted && errorCode === "client_cancelled") {
        return fakeCancelled(admittedRequestId);
      }
      return {
        outcome: "failed",
        correct: false,
        expected_outcome_met: false,
        status_code: controller.signal.aborted ? 504 : 502,
        error_code: errorCode,
        request_id: admittedRequestId,
        request_ids: admittedRequestId === null ? [] : [admittedRequestId],
        provider: null,
        model: null,
        reasoning_effort: null,
        auth_mode: null,
        codex_cli_version: null,
        frontier_name: workload.expected_frontier,
        worker_duration_ms: null,
        calls: [],
        usage: zeroUsage(),
        product: null,
      };
    } finally {
      clearTimeout(timer);
      linked.unlink();
    }
  };
}

async function executeProduct(workload, context, productExecutor) {
  if (typeof productExecutor !== "function") {
    throw new SloRunError("product_executor_required");
  }
  const product = validateProductResult(workload, await runWithSignal(
    context.signal,
    () => productExecutor({
      case_id: workload.case_id,
      sequence: context.sequence,
      expected_model_calls: workload.required_model_calls,
      signal: context.signal,
    }),
  ));
  const usage = usageFromCalls(product.calls);
  return {
    outcome: "completed",
    correct: true,
    expected_outcome_met: true,
    status_code: 200,
    error_code: null,
    request_id: null,
    request_ids: product.calls.map((call) => call.request_id),
    provider: context.plan.identity.provider,
    model: context.plan.identity.model,
    reasoning_effort: context.plan.identity.reasoning_effort,
    auth_mode: context.plan.identity.auth_mode,
    codex_cli_version: context.plan.identity.codex_cli_version,
    frontier_name: product.calls.map((call) => call.frontier_name).join(","),
    worker_duration_ms: null,
    calls: product.calls,
    usage,
    product: {
      case_id: product.case_id,
      exact_semantics: product.exact_semantics,
      validation_current: product.validation_current,
      simulation_current: product.simulation_current,
      candidate_only: product.candidate_only,
    },
  };
}

function stopReason(observations, usage, plan, elapsedMs) {
  const unexpected = observations.find((observation) => !observation.expected_outcome_met);
  if (unexpected) {
    return unexpected.error_code ?? "unexpected_outcome";
  }
  if (usage.input_tokens > plan.budgets.input_tokens) {
    return "input_token_budget_exceeded";
  }
  if (usage.output_tokens > plan.budgets.output_tokens) {
    return "output_token_budget_exceeded";
  }
  if (elapsedMs > plan.budgets.max_duration_ms) {
    return "duration_budget_exceeded";
  }
  return null;
}

export async function runPlan(options) {
  const plan = assertPlanBudget(assertKnownPlan(structuredClone(options.plan)));
  const source = validateSource(options.source, plan.execution_mode === "live");
  const runId = validateRunId(options.runId ?? `slo-${randomUUID()}`);
  const fetchFn = options.fetchFn ?? globalThis.fetch;
  const clock = options.clock ?? {
    now: () => performance.now(),
    sleep: (milliseconds) => new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds)),
  };
  const wallClock = options.wallClock ?? (() => new Date().toISOString());
  const healthPollMs = options.healthPollMs ?? 25;
  const cancellationAdmissionTimeoutMs = options.cancellationAdmissionTimeoutMs ?? 5_000;
  const metricsHealthTimeoutMs = options.metricsHealthTimeoutMs ?? 5_000;
  const resourceCleanupTimeoutMs = options.resourceCleanupTimeoutMs ?? 5_000;
  const deadlineScheduler = options.scheduleRunDeadline ?? scheduleRunDeadline;
  if (!Number.isSafeInteger(healthPollMs) || healthPollMs < 1
    || !Number.isSafeInteger(cancellationAdmissionTimeoutMs)
    || cancellationAdmissionTimeoutMs < 1
    || !Number.isSafeInteger(metricsHealthTimeoutMs) || metricsHealthTimeoutMs < 1
    || !Number.isSafeInteger(resourceCleanupTimeoutMs)
    || resourceCleanupTimeoutMs < 1
    || resourceCleanupTimeoutMs > 60_000
    || typeof deadlineScheduler !== "function") {
    throw new SloRunError("invalid_runner_timing");
  }

  let readHealth;
  let readMetricsHealth = null;
  let executeTransport;
  let liveTransportConfig = null;
  let fakeBoundary = null;
  if (plan.execution_mode === "live") {
    const baseUrl = validateLoopbackBaseUrl(options.baseUrl);
    if (typeof options.token !== "string"
      || options.token.length < 12
      || options.token !== options.token.trim()) {
      throw new SloRunError("worker_token_required");
    }
    if (typeof fetchFn !== "function") {
      throw new SloRunError("fetch_required");
    }
    readHealth = createLiveHealthReader(baseUrl, options.token, fetchFn);
    readMetricsHealth = createLiveMetricsHealthReader(baseUrl, options.token, fetchFn);
    liveTransportConfig = { baseUrl, token: options.token, fetchFn };
  } else {
    fakeBoundary = createFakeBoundary(plan);
    readHealth = options.healthReader ?? fakeBoundary.health;
    executeTransport = options.fakeExecutor ?? (async (workload, sequence) => (
      workload.executor === "worker_http_cancel"
        ? fakeCancelled()
        : fakeCompleted(sequence, plan)
    ));
  }

  const startedAtWall = wallClock();
  const startedAt = clock.now();
  const runController = new AbortController();
  const runSignal = runController.signal;
  const observations = [];
  const waves = [];
  const healthSamples = [];
  const metricsHealthSamples = [];
  let usage = zeroUsage();
  let liveCalls = 0;
  let liveCallCountKnown = true;
  let stop = null;
  let boundary = null;
  let resourceResult = null;
  let workerMetrics = [];
  const suppliedScenarios = sanitizeScenarios(options.scenarioResults);

  const sampleHealth = async (stage, signal = runSignal) => {
    const valid = validateWorkerHealth(await readHealth(signal), plan, boundary);
    if (plan.execution_mode === "live"
      && valid.worker_source_sha256 !== source.worker_source_sha256) {
      throw new SloRunError("worker_source_mismatch");
    }
    const previous = healthSamples.at(-1);
    if (previous
      && (valid.accepted_requests_total < previous.accepted_requests_total
        || valid.settled_requests_total < previous.settled_requests_total)) {
      throw new SloRunError("worker_counter_discontinuity");
    }
    if (boundary === null) {
      boundary = valid;
    }
    healthSamples.push({
      at_ms: finiteDuration(clock.now() - startedAt),
      stage,
      ...valid,
    });
    return valid;
  };

  const sampleMetricsHealth = async (stage, signal = runSignal) => {
    if (readMetricsHealth === null) {
      return null;
    }
    const value = validateMetricsHealth(await readMetricsHealth(signal), boundary);
    const previous = metricsHealthSamples.at(-1);
    if (previous
      && (value.records_attempted < previous.records_attempted
        || value.records_written < previous.records_written
        || value.write_failures_total < previous.write_failures_total)) {
      throw new SloRunError("metrics_counter_discontinuity");
    }
    metricsHealthSamples.push({
      at_ms: finiteDuration(clock.now() - startedAt),
      stage,
      ...value,
    });
    return value;
  };

  if (liveTransportConfig) {
    executeTransport = createLiveTransportExecutor(
      liveTransportConfig.baseUrl,
      liveTransportConfig.token,
      liveTransportConfig.fetchFn,
      plan.worker_profile.request_timeout_ms,
      cancellationAdmissionTimeoutMs,
      healthPollMs,
      clock,
      runSignal,
    );
  }

  const cancelRunDeadline = deadlineScheduler(
    runController,
    plan.budgets.max_duration_ms,
  );
  if (typeof cancelRunDeadline !== "function") {
    throw new SloRunError("invalid_deadline_scheduler");
  }

  const resourceSampler = options.resourceSampler ?? null;
  let resourceSamplerCleanupRequired = false;

  try {
    if (resourceSampler) {
      resourceSamplerCleanupRequired = true;
      await runWithSignal(runSignal, () => resourceSampler.start(runSignal));
    }
    const initial = await sampleHealth("initial");
    if (initial.active_requests !== 0
      || initial.queued_requests !== 0
      || initial.accepted_requests_total !== initial.settled_requests_total) {
      throw new SloRunError("worker_not_idle");
    }
    const initialMetrics = await sampleMetricsHealth("initial");
    if (initialMetrics
      && (initialMetrics.status !== "ok"
        || !initialMetrics.writable_verified
        || initialMetrics.pending_records !== 0
        || initialMetrics.write_failures_total !== 0
        || initialMetrics.last_error_code !== null)) {
      throw new SloRunError("metrics_unavailable");
    }

    let sequenceNumber = 0;
    for (const phase of plan.phases) {
      if (stop !== null) {
        break;
      }
      const workload = getWorkload(phase.workload_id);
      if (workload.calls_per_invocation !== phase.calls_per_invocation) {
        throw new SloRunError("plan_workload_call_mismatch");
      }
      for (let waveIndex = 0; waveIndex < phase.waves; waveIndex += 1) {
        const plannedCalls = phase.concurrency * phase.calls_per_invocation;
        if (plan.execution_mode === "live"
          && liveCalls + plannedCalls > plan.budgets.live_calls) {
          stop = "live_call_budget_exceeded";
          break;
        }
        const waveStarted = clock.now();
        const admissionSnapshot = await sampleHealth(`before:${phase.id}:${waveIndex}`);
        let waveFinished = false;
        let pollError = null;
        const poller = (async () => {
          if (workload.executor === "worker_http_cancel") {
            return;
          }
          while (!waveFinished) {
            try {
              await runWithSignal(runSignal, () => clock.sleep(healthPollMs));
            } catch (error) {
              pollError = error;
              return;
            }
            if (!waveFinished) {
              try {
                await sampleHealth(`during:${phase.id}:${waveIndex}`);
              } catch (error) {
                pollError = error;
                return;
              }
            }
          }
        })();
        const invocationPromises = Array.from({ length: phase.concurrency }, async (_, slotIndex) => {
          sequenceNumber += 1;
          const sequence = `${runId}-${String(sequenceNumber).padStart(4, "0")}`;
          const dispatched = clock.now();
          let result;
          try {
            if (workload.executor === "product_adapter") {
              result = await executeProduct(
                workload,
                { sequence, plan, signal: runSignal },
                options.productExecutor,
              );
            } else {
              result = await executeTransport(workload, sequence, {
                admissionSnapshot,
                sampleHealth,
                phaseId: phase.id,
                waveIndex,
              });
            }
          } catch (error) {
            const errorCode = safeErrorCode(error?.code);
            result = {
              outcome: "failed",
              correct: false,
              expected_outcome_met: false,
              status_code: errorCode === "duration_budget_exceeded" ? 504 : 502,
              error_code: errorCode,
              request_id: null,
              request_ids: [],
              provider: null,
              model: null,
              reasoning_effort: null,
              auth_mode: null,
              codex_cli_version: null,
              frontier_name: null,
              worker_duration_ms: null,
              calls: [],
              usage: zeroUsage(),
              product: null,
            };
          }
          const completed = clock.now();
          if (plan.execution_mode === "live") {
            if (workload.executor === "product_adapter") {
              if (result.outcome === "completed"
                && result.calls.length === phase.calls_per_invocation) {
                liveCalls += result.calls.length;
              } else {
                liveCallCountKnown = false;
              }
            } else {
              liveCalls += phase.calls_per_invocation;
            }
          } else {
            fakeBoundary?.settle(phase.calls_per_invocation);
          }
          return {
            schema_version: 1,
            sequence,
            phase_id: phase.id,
            workload_id: workload.id,
            wave_index: waveIndex,
            slot_index: slotIndex,
            concurrency: phase.concurrency,
            warmup: phase.warmup,
            expected_outcome: phase.expected_outcome,
            attempt: 1,
            provider_invoked: plan.execution_mode === "live",
            planned_calls: phase.calls_per_invocation,
            dispatch_offset_ms: finiteDuration(dispatched - startedAt),
            completion_offset_ms: finiteDuration(completed - startedAt),
            latency_ms: finiteDuration(completed - dispatched),
            ...result,
          };
        });
        const waveObservations = await Promise.all(invocationPromises);
        waveFinished = true;
        await poller;
        if (pollError) {
          stop = safeErrorCode(pollError.code, "health_sampling_failed");
        }
        observations.push(...waveObservations);
        for (const observation of waveObservations) {
          usage = addUsage(usage, observation.usage);
        }
        try {
          await sampleHealth(`after:${phase.id}:${waveIndex}`);
        } catch (error) {
          stop ??= safeErrorCode(error.code, "health_sampling_failed");
        }
        const waveEnded = clock.now();
        waves.push({
          phase_id: phase.id,
          workload_id: workload.id,
          wave_index: waveIndex,
          concurrency: phase.concurrency,
          start_offset_ms: finiteDuration(waveStarted - startedAt),
          end_offset_ms: finiteDuration(waveEnded - startedAt),
          duration_ms: finiteDuration(waveEnded - waveStarted),
          observations: waveObservations.length,
          correct_completions: waveObservations.filter((row) => row.correct).length,
          expected_outcomes_met: waveObservations.filter((row) => row.expected_outcome_met).length,
        });
        stop ??= stopReason(waveObservations, usage, plan, waveEnded - startedAt);
        if (phase.expected_outcome === "cancelled" && stop === null) {
          const recoveryStarted = clock.now();
          let recovered = false;
          while (clock.now() - recoveryStarted <= 5_000) {
            const health = await sampleHealth("cancellation_recovery");
            if (health.active_requests === 0
              && health.queued_requests === 0
              && health.accepted_requests_total === health.settled_requests_total) {
              recovered = true;
              break;
            }
            await runWithSignal(runSignal, () => clock.sleep(healthPollMs));
          }
          const cancellation = waveObservations[0];
          cancellation.cancellation_recovery_ms = finiteDuration(clock.now() - recoveryStarted);
          cancellation.cancellation_recovered = recovered;
          if (!recovered) {
            stop = "cancellation_recovery_timeout";
          }
        }
        if (stop !== null) {
          break;
        }
      }
    }
  } catch (error) {
    stop ??= safeErrorCode(error?.code, "run_failed");
  } finally {
    try {
      await sampleHealth("final");
    } catch (error) {
      stop ??= safeErrorCode(error?.code, "final_health_failed");
    }
    if (resourceSamplerCleanupRequired) {
      const cleanupController = new AbortController();
      const cleanupTimer = setTimeout(
        () => cleanupController.abort(new SloRunError("resource_sampler_cleanup_timeout")),
        resourceCleanupTimeoutMs,
      );
      try {
        resourceResult = await runWithSignal(
          cleanupController.signal,
          () => resourceSampler.stop(cleanupController.signal),
          "resource_sampler_cleanup_timeout",
        );
      } catch (error) {
        stop ??= safeErrorCode(error?.code, "resource_sampler_failed");
      } finally {
        clearTimeout(cleanupTimer);
      }
    }
  }

  if (plan.execution_mode === "live") {
    const metricsStart = metricsHealthSamples[0] ?? null;
    const expectedDelta = Number.isSafeInteger(healthSamples.at(-1)?.accepted_requests_total)
      && Number.isSafeInteger(healthSamples[0]?.accepted_requests_total)
      ? healthSamples.at(-1).accepted_requests_total - healthSamples[0].accepted_requests_total
      : null;
    let metricsSettled = false;
    const metricsDeadline = clock.now() + metricsHealthTimeoutMs;
    let metricsSampleCount = 0;
    while (metricsStart !== null && expectedDelta !== null && clock.now() <= metricsDeadline) {
      try {
        const current = await sampleMetricsHealth(metricsSampleCount === 0 ? "final" : "correlation");
        metricsSampleCount += 1;
        const attemptedDelta = current.records_attempted - metricsStart.records_attempted;
        const writtenDelta = current.records_written - metricsStart.records_written;
        if (current.status !== "ok"
          || !current.writable_verified
          || current.write_failures_total !== 0
          || current.last_error_code !== null
          || attemptedDelta > expectedDelta
          || writtenDelta > expectedDelta) {
          stop ??= "metrics_unavailable";
          break;
        }
        if (current.pending_records === 0
          && attemptedDelta === expectedDelta
          && writtenDelta === expectedDelta) {
          metricsSettled = true;
          break;
        }
      } catch {
        stop ??= "metrics_unavailable";
        break;
      }
      try {
        await runWithSignal(runSignal, () => clock.sleep(healthPollMs));
      } catch {
        stop ??= "duration_budget_exceeded";
        break;
      }
    }
    if (!metricsSettled) {
      stop ??= "metrics_unavailable";
    }
    if (typeof options.metricsReader !== "function") {
      stop ??= "metrics_unavailable";
    } else {
      try {
        const records = await runWithSignal(runSignal, () => options.metricsReader({
          request_ids: observations
            .flatMap((observation) => observation.request_ids)
            .filter((requestId) => typeof requestId === "string"),
          worker_boundary: boundary ? {
            instance_id: boundary.instance_id,
            worker_source_sha256: boundary.worker_source_sha256,
          } : null,
          start_counters: {
            accepted: healthSamples[0]?.accepted_requests_total ?? null,
            settled: healthSamples[0]?.settled_requests_total ?? null,
          },
          end_counters: {
            accepted: healthSamples.at(-1)?.accepted_requests_total ?? null,
            settled: healthSamples.at(-1)?.settled_requests_total ?? null,
          },
          metrics_health_start: metricsHealthSamples[0] ? {
            records_attempted: metricsHealthSamples[0].records_attempted,
            records_written: metricsHealthSamples[0].records_written,
          } : null,
          metrics_health_end: metricsHealthSamples.at(-1) ? {
            records_attempted: metricsHealthSamples.at(-1).records_attempted,
            records_written: metricsHealthSamples.at(-1).records_written,
          } : null,
          expected_records: expectedDelta,
          signal: runSignal,
        }));
        if (!Array.isArray(records)) {
          throw new SloRunError("invalid_worker_metrics");
        }
        workerMetrics = records.map(sanitizeWorkerMetric);
      } catch {
        stop ??= "metrics_unavailable";
      }
    }
  }
  const cancellation = observations.find((row) => row.expected_outcome === "cancelled");
  const scenarios = suppliedScenarios.filter((entry) => entry.id !== "cancellation");
  if (cancellation) {
    scenarios.push({
      id: "cancellation",
      status: cancellation.expected_outcome_met && cancellation.cancellation_recovered
        ? "pass"
        : "fail",
      duration_ms: cancellation.cancellation_recovery_ms ?? cancellation.latency_ms,
      error_code: cancellation.expected_outcome_met && cancellation.cancellation_recovered
        ? null
        : cancellation.error_code ?? "cancellation_recovery_failed",
    });
  }
  cancelRunDeadline();
  const completedAt = clock.now();
  const finalHealth = healthSamples.at(-1) ?? null;
  const initialHealth = healthSamples[0] ?? null;
  return {
    schema_version: 1,
    run_id: runId,
    plan,
    plan_digest: planDigest(plan),
    source,
    execution_mode: plan.execution_mode,
    started_at: startedAtWall,
    completed_at: wallClock(),
    duration_ms: finiteDuration(completedAt - startedAt),
    interrupted: stop !== null,
    stop_reason: stop,
    automatic_retries: 0,
    planned_live_calls: planLiveCallCount(plan),
    observed_live_calls: liveCalls,
    live_call_count_known: liveCallCountKnown,
    usage,
    worker_boundary: boundary ? {
      instance_id: boundary.instance_id,
      worker_source_sha256: boundary.worker_source_sha256,
      identity: Object.fromEntries(Object.keys(plan.identity).map((key) => [key, boundary[key]])),
      profile: Object.fromEntries(
        Object.keys(plan.worker_profile).map((key) => [key, boundary[key]]),
      ),
    } : null,
    counters: {
      start_accepted: initialHealth?.accepted_requests_total ?? null,
      start_settled: initialHealth?.settled_requests_total ?? null,
      end_accepted: finalHealth?.accepted_requests_total ?? null,
      end_settled: finalHealth?.settled_requests_total ?? null,
    },
    health_samples: healthSamples,
    metrics_health_samples: metricsHealthSamples,
    resource_samples: resourceResult?.samples ?? [],
    resource_errors: resourceResult?.errors ?? [],
    resource_duration_ms: resourceResult?.duration_ms ?? 0,
    worker_metrics: workerMetrics,
    scenarios: scenarios.sort((left, right) => left.id.localeCompare(right.id)),
    waves,
    observations,
  };
}
