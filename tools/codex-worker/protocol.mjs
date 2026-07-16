export const SCHEMA_VERSION = 1;
export const PROVIDER = "codex_chatgpt";
export const MODEL = "gpt-5.6-luna";
export const REASONING_EFFORT = "medium";
export const AUTH_MODE = "chatgpt";
export const CODEX_CLI_VERSION = "codex-cli 0.144.2";

const REQUEST_KEYS = [
  "schema_version",
  "model",
  "reasoning_effort",
  "messages",
  "frontier",
];
const FRONTIER_KEYS = ["name", "description", "parameters"];
const MESSAGE_REQUIRED_KEYS = ["role", "content"];
const MESSAGE_OPTIONAL_KEYS = ["tool_call_id", "tool_calls"];
const TOOL_CALL_KEYS = ["id", "name", "arguments"];
const ROLES = new Set(["system", "user", "assistant", "tool"]);

export class WorkerError extends Error {
  constructor(code, status) {
    super(code);
    this.name = "WorkerError";
    this.code = code;
    this.status = status;
  }
}

export function isPlainObject(value) {
  return value !== null
    && typeof value === "object"
    && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

function hasExactKeys(value, required, optional = []) {
  const keys = Object.keys(value).sort();
  const allowed = new Set([...required, ...optional]);
  return required.every((key) => Object.hasOwn(value, key))
    && keys.every((key) => allowed.has(key));
}

function nonEmptyString(value, maxLength) {
  return typeof value === "string"
    && value.length > 0
    && value.length <= maxLength
    && value === value.trim();
}

function validateToolCall(value) {
  if (!isPlainObject(value) || !hasExactKeys(value, TOOL_CALL_KEYS)) {
    throw new WorkerError("invalid_message_tool_call", 400);
  }
  if (!nonEmptyString(value.id, 256)
    || !nonEmptyString(value.name, 128)
    || typeof value.arguments !== "string") {
    throw new WorkerError("invalid_message_tool_call", 400);
  }
  try {
    JSON.parse(value.arguments);
  } catch {
    throw new WorkerError("invalid_message_tool_call", 400);
  }
}

function validateMessage(value) {
  if (!isPlainObject(value)
    || !hasExactKeys(value, MESSAGE_REQUIRED_KEYS, MESSAGE_OPTIONAL_KEYS)
    || !ROLES.has(value.role)
    || typeof value.content !== "string") {
    throw new WorkerError("invalid_message", 400);
  }
  if (value.content.length > 1_000_000) {
    throw new WorkerError("invalid_message", 400);
  }
  if (Object.hasOwn(value, "tool_call_id")
    && value.tool_call_id !== null
    && !nonEmptyString(value.tool_call_id, 256)) {
    throw new WorkerError("invalid_message", 400);
  }
  if (Object.hasOwn(value, "tool_calls")) {
    if (!Array.isArray(value.tool_calls) || value.tool_calls.length > 32) {
      throw new WorkerError("invalid_message", 400);
    }
    value.tool_calls.forEach(validateToolCall);
  }
}

function validateFrontier(value) {
  if (!isPlainObject(value) || !hasExactKeys(value, FRONTIER_KEYS)) {
    throw new WorkerError("invalid_frontier", 400);
  }
  if (typeof value.name !== "string"
    || !/^[a-z][a-z0-9_]{0,127}$/.test(value.name)
    || typeof value.description !== "string"
    || value.description.length > 8_192
    || !isPlainObject(value.parameters)) {
    throw new WorkerError("invalid_frontier", 400);
  }
  if (value.parameters.type !== "object") {
    throw new WorkerError("invalid_frontier_schema", 400);
  }
}

export function validateCompletionRequest(value) {
  if (!isPlainObject(value) || !hasExactKeys(value, REQUEST_KEYS)) {
    throw new WorkerError("invalid_request_shape", 400);
  }
  if (value.schema_version !== SCHEMA_VERSION) {
    throw new WorkerError("unsupported_schema_version", 400);
  }
  if (value.model !== MODEL || value.reasoning_effort !== REASONING_EFFORT) {
    throw new WorkerError("identity_mismatch", 400);
  }
  if (!Array.isArray(value.messages)
    || value.messages.length === 0
    || value.messages.length > 512) {
    throw new WorkerError("invalid_messages", 400);
  }
  value.messages.forEach(validateMessage);
  validateFrontier(value.frontier);
  return value;
}

export function validateUsage(value) {
  const fields = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
  ];
  if (!isPlainObject(value) || !hasExactKeys(value, fields)) {
    throw new WorkerError("invalid_usage", 502);
  }
  for (const field of fields) {
    if (!Number.isSafeInteger(value[field]) || value[field] < 0) {
      throw new WorkerError("invalid_usage", 502);
    }
  }
  if (value.cached_input_tokens > value.input_tokens
    || value.reasoning_output_tokens > value.output_tokens) {
    throw new WorkerError("invalid_usage", 502);
  }
  return value;
}

export function validateRunnerResult(value, identity) {
  if (!isPlainObject(value)
    || value.model !== MODEL
    || value.reasoning_effort !== REASONING_EFFORT
    || value.auth_mode !== AUTH_MODE
    || value.codex_cli_version !== identity.codex_cli_version) {
    throw new WorkerError("provider_identity_mismatch", 502);
  }
  if (typeof value.arguments !== "string") {
    throw new WorkerError("invalid_structured_output", 502);
  }
  let argumentsValue;
  try {
    argumentsValue = JSON.parse(value.arguments);
  } catch {
    throw new WorkerError("invalid_structured_output", 502);
  }
  if (!isPlainObject(argumentsValue)) {
    throw new WorkerError("invalid_structured_output", 502);
  }
  validateUsage(value.usage);
  return value;
}

export function healthEnvelope(
  identity,
  active,
  queued,
  concurrencyLimit,
  queueCapacity,
  requestTimeoutMs,
) {
  return {
    schema_version: SCHEMA_VERSION,
    status: "ok",
    provider: PROVIDER,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    auth_mode: AUTH_MODE,
    codex_cli_version: identity.codex_cli_version,
    instance_id: identity.instance_id,
    worker_source_sha256: identity.worker_source_sha256,
    concurrency_limit: concurrencyLimit,
    queue_capacity: queueCapacity,
    request_timeout_ms: requestTimeoutMs,
    active_requests: active,
    queued_requests: queued,
  };
}

export function completionEnvelope(identity, requestId, frontierName, result, durationMs) {
  return {
    schema_version: SCHEMA_VERSION,
    request_id: requestId,
    provider: PROVIDER,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    auth_mode: AUTH_MODE,
    codex_cli_version: identity.codex_cli_version,
    tool_call: {
      id: `call-${requestId}`,
      name: frontierName,
      arguments: result.arguments,
    },
    usage: result.usage,
    duration_ms: durationMs,
  };
}
