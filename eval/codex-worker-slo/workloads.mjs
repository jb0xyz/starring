import { EXPECTED_IDENTITY } from "./plans.mjs";

const WORKLOADS = {
  transport_probe: {
    schema_version: 1,
    id: "transport_probe",
    layer: "worker_transport",
    executor: "worker_http",
    calls_per_invocation: 1,
    expected_frontier: "record_slo_probe",
  },
  transport_cancellation: {
    schema_version: 1,
    id: "transport_cancellation",
    layer: "failure_recovery",
    executor: "worker_http_cancel",
    calls_per_invocation: 1,
    expected_frontier: "record_slo_probe",
  },
  starring_v15_one_call: {
    schema_version: 1,
    id: "starring_v15_one_call",
    layer: "starring_authoring",
    executor: "product_adapter",
    calls_per_invocation: 1,
    case_id: "intent_private_study_room_en",
    required_model_calls: 1,
    require_validation: true,
    require_simulation: true,
    require_candidate_only: true,
  },
  starring_v15_two_call: {
    schema_version: 1,
    id: "starring_v15_two_call",
    layer: "starring_authoring",
    executor: "product_adapter",
    calls_per_invocation: 2,
    case_id: "intent_private_study_room_custom_details",
    required_model_calls: 2,
    require_validation: true,
    require_simulation: true,
    require_candidate_only: true,
  },
};

for (const workload of Object.values(WORKLOADS)) {
  Object.freeze(workload);
}

export class WorkloadError extends Error {
  constructor(code) {
    super(code);
    this.name = "WorkloadError";
    this.code = code;
  }
}

function exactKeys(value, keys) {
  return value
    && typeof value === "object"
    && !Array.isArray(value)
    && Object.keys(value).sort().join("\u0000") === [...keys].sort().join("\u0000");
}

function nonNegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function validRequestId(value) {
  return typeof value === "string"
    && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(value);
}

function validateUsage(usage) {
  const keys = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
  ];
  if (!exactKeys(usage, keys) || !keys.every((key) => nonNegativeInteger(usage[key]))) {
    throw new WorkloadError("invalid_usage");
  }
  if (usage.cached_input_tokens > usage.input_tokens
    || usage.reasoning_output_tokens > usage.output_tokens) {
    throw new WorkloadError("invalid_usage");
  }
  return structuredClone(usage);
}

export function workloadIds() {
  return Object.keys(WORKLOADS);
}

export function getWorkload(id) {
  const workload = WORKLOADS[id];
  if (!workload) {
    throw new WorkloadError("unknown_workload");
  }
  return structuredClone(workload);
}

export function buildTransportRequest(sequence) {
  if (typeof sequence !== "string" || !/^[a-z0-9][a-z0-9_-]{0,127}$/.test(sequence)) {
    throw new WorkloadError("invalid_probe_sequence");
  }
  return {
    schema_version: 1,
    model: EXPECTED_IDENTITY.model,
    reasoning_effort: EXPECTED_IDENTITY.reasoning_effort,
    messages: [
      {
        role: "system",
        content: "Call the sole frontier exactly once with the requested fixed values.",
      },
      {
        role: "user",
        content: `Set sequence to ${sequence} and status to ok.`,
      },
    ],
    frontier: {
      name: "record_slo_probe",
      description: "Return the fixed SLO transport probe sequence and status.",
      parameters: {
        type: "object",
        properties: {
          schema_version: { type: "integer", const: 1 },
          sequence: { type: "string", const: sequence },
          status: { type: "string", const: "ok" },
        },
        required: ["schema_version", "sequence", "status"],
        additionalProperties: false,
      },
    },
  };
}

export function validateTransportCompletion(statusCode, body, sequence) {
  if (statusCode !== 200 || !body || typeof body !== "object" || Array.isArray(body)) {
    throw new WorkloadError("transport_http_failure");
  }
  if (body.schema_version !== 1
    || body.provider !== EXPECTED_IDENTITY.provider
    || body.model !== EXPECTED_IDENTITY.model
    || body.reasoning_effort !== EXPECTED_IDENTITY.reasoning_effort
    || body.auth_mode !== EXPECTED_IDENTITY.auth_mode
    || body.codex_cli_version !== EXPECTED_IDENTITY.codex_cli_version) {
    throw new WorkloadError("transport_identity_mismatch");
  }
  if (typeof body.request_id !== "string" || body.request_id.length === 0
    || !body.tool_call || body.tool_call.name !== "record_slo_probe"
    || typeof body.tool_call.id !== "string"
    || typeof body.tool_call.arguments !== "string") {
    throw new WorkloadError("transport_invalid_response");
  }
  let args;
  try {
    args = JSON.parse(body.tool_call.arguments);
  } catch {
    throw new WorkloadError("transport_invalid_arguments");
  }
  if (!exactKeys(args, ["schema_version", "sequence", "status"])
    || args.schema_version !== 1
    || args.sequence !== sequence
    || args.status !== "ok") {
    throw new WorkloadError("transport_wrong_arguments");
  }
  if (!nonNegativeInteger(body.duration_ms)) {
    throw new WorkloadError("transport_invalid_duration");
  }
  return {
    request_id: body.request_id,
    provider: body.provider,
    model: body.model,
    reasoning_effort: body.reasoning_effort,
    auth_mode: body.auth_mode,
    codex_cli_version: body.codex_cli_version,
    frontier_name: body.tool_call.name,
    worker_duration_ms: body.duration_ms,
    usage: validateUsage(body.usage),
  };
}

function safeProductCall(call) {
  const required = [
    "request_id",
    "status_code",
    "latency_ms",
    "provider",
    "model",
    "reasoning_effort",
    "auth_mode",
    "codex_cli_version",
    "frontier_name",
    "usage",
  ];
  if (!exactKeys(call, required)
    || !validRequestId(call.request_id)
    || call.status_code !== 200
    || !Number.isFinite(call.latency_ms)
    || call.latency_ms < 0
    || call.provider !== EXPECTED_IDENTITY.provider
    || call.model !== EXPECTED_IDENTITY.model
    || call.reasoning_effort !== EXPECTED_IDENTITY.reasoning_effort
    || call.auth_mode !== EXPECTED_IDENTITY.auth_mode
    || call.codex_cli_version !== EXPECTED_IDENTITY.codex_cli_version
    || typeof call.frontier_name !== "string"
    || call.frontier_name.length === 0) {
    throw new WorkloadError("invalid_product_call");
  }
  return {
    ...call,
    usage: validateUsage(call.usage),
  };
}

export function validateProductResult(workload, result) {
  if (workload.executor !== "product_adapter") {
    throw new WorkloadError("not_product_workload");
  }
  const keys = [
    "case_id",
    "calls",
    "exact_semantics",
    "validation_current",
    "simulation_current",
    "candidate_only",
  ];
  if (!exactKeys(result, keys)
    || result.case_id !== workload.case_id
    || !Array.isArray(result.calls)
    || result.calls.length !== workload.required_model_calls
    || result.exact_semantics !== true
    || result.validation_current !== workload.require_validation
    || result.simulation_current !== workload.require_simulation
    || result.candidate_only !== workload.require_candidate_only) {
    throw new WorkloadError("invalid_product_result");
  }
  const calls = result.calls.map(safeProductCall);
  if (new Set(calls.map((call) => call.request_id)).size !== calls.length) {
    throw new WorkloadError("duplicate_product_request_id");
  }
  return {
    case_id: result.case_id,
    calls,
    exact_semantics: result.exact_semantics,
    validation_current: result.validation_current,
    simulation_current: result.simulation_current,
    candidate_only: result.candidate_only,
  };
}
