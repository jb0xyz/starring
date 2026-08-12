import { randomUUID, webcrypto } from "node:crypto";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import vm from "node:vm";


const MAX_INPUT_BYTES = 64 * 1024;
const MAX_OUTPUT_BYTES = 64 * 1024;
const MAX_RESPONSE_BYTES = 512 * 1024;
const MAX_SCENARIO_MESSAGE_BYTES = 16 * 1024;
const PRODUCT_DRIVER_URL = new URL("../d2-certification/product_driver.js", import.meta.url);
const D2_ORIGIN = "https://d2-api.starring.co.kr";
const SESSION_SECRET = /^[A-Za-z0-9_-]{43}$/;
const DIGEST = /^[0-9a-f]{64}$/;
const RESOURCE_ID = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
const AUTHORING_SESSION_PREFIX = /^[A-Za-z0-9][A-Za-z0-9._-]{0,110}$/;
const SNOWFLAKE = /^[1-9][0-9]{0,19}$/;
const RUN_ID = /^d2-[0-9]{8}t[0-9]{6}z-[0-9a-f]{12}$/;
const UTC_TIMESTAMP = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$/;
const PROCESS_INSTANCE_ID = /^[0-9a-f]{32}$/;
const OPERATIONS = new Set(["auth-smoke", "one-shot"]);
const ROOT_FIELDS = new Set([
  "schema_version",
  "session",
  "csrf",
  "public_origin",
  "principal_id",
  "guild_id",
  "installation_id",
  "run_id",
  "manifest_sha256",
  "operation",
  "scenario",
  "scenario_sha256",
  "authoring_session_id",
]);
const SCENARIO_FIELDS = new Set([
  "schema_version",
  "kind",
  "session_id_prefix",
  "message",
  "expected_generation",
  "expected_summary",
]);
const SUMMARY_FIELDS = new Set([
  "panels",
  "modals",
  "rules",
  "actions",
  "target_version",
  "required_approvals",
]);
const UNCOVERED_RELEASE_BOUNDARIES = Object.freeze([
  "discord_oauth_consent_and_code_exchange",
  "real_discord_gateway_interactions",
  "discord_web_confirmation_surface",
  "human_preview_approval",
  "disposable_guild_deletion",
]);


class RunnerFailure extends Error {
  constructor(code) {
    super(code);
    this.name = "StarringD2AHeadlessRunnerFailure";
    this.code = code;
  }
}


function fail(code) {
  throw new RunnerFailure(code);
}


function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}


function requireExactFields(value, allowed, required, label) {
  if (!isObject(value)) {
    fail(`${label}_invalid`);
  }
  const keys = Object.keys(value);
  if (
    keys.some((key) => !allowed.has(key)) ||
    required.some((key) => !Object.hasOwn(value, key))
  ) {
    fail(`${label}_invalid`);
  }
  return value;
}


function requireString(value, pattern, label) {
  if (typeof value !== "string" || !pattern.test(value)) {
    fail(`${label}_invalid`);
  }
  return value;
}


function requireSecret(value, label) {
  requireString(value, SESSION_SECRET, label);
  let decoded;
  try {
    decoded = Buffer.from(value, "base64url");
  } catch {
    fail(`${label}_invalid`);
  }
  if (decoded.length !== 32 || decoded.toString("base64url") !== value) {
    fail(`${label}_invalid`);
  }
  return value;
}


function requireInteger(value, minimum, maximum, label) {
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    fail(`${label}_invalid`);
  }
  return value;
}


function requireBoolean(value, label) {
  if (typeof value !== "boolean") {
    fail(`${label}_invalid`);
  }
  return value;
}


function requireChoice(value, choices, label) {
  if (typeof value !== "string" || !choices.has(value)) {
    fail(`${label}_invalid`);
  }
  return value;
}


function requireSummary(value, label) {
  const summary = requireExactFields(
    value,
    SUMMARY_FIELDS,
    [...SUMMARY_FIELDS],
    label,
  );
  const normalized = {};
  for (const field of ["panels", "modals", "rules", "actions"]) {
    normalized[field] = requireInteger(summary[field], 0, Number.MAX_SAFE_INTEGER, label);
  }
  normalized.target_version = requireInteger(
    summary.target_version,
    1,
    Number.MAX_SAFE_INTEGER,
    label,
  );
  if (summary.required_approvals !== 1) {
    fail(`${label}_invalid`);
  }
  normalized.required_approvals = 1;
  return Object.freeze(normalized);
}


function requireScenario(value) {
  const scenario = requireExactFields(
    value,
    SCENARIO_FIELDS,
    ["schema_version", "kind", "session_id_prefix", "message", "expected_generation", "expected_summary"],
    "scenario",
  );
  if (
    scenario.schema_version !== 1 ||
    scenario.kind !== "starring.d2a.product-scenario.v1"
  ) {
    fail("scenario_contract_invalid");
  }
  if (
    typeof scenario.message !== "string" ||
    Buffer.byteLength(scenario.message, "utf8") < 1 ||
    Buffer.byteLength(scenario.message, "utf8") > MAX_SCENARIO_MESSAGE_BYTES
  ) {
    fail("scenario_message_invalid");
  }
  const normalized = {
    schema_version: 1,
    kind: "starring.d2a.product-scenario.v1",
    session_id_prefix: requireString(
      scenario.session_id_prefix,
      AUTHORING_SESSION_PREFIX,
      "scenario_session_id_prefix",
    ),
    message: scenario.message,
    expected_generation: requireInteger(scenario.expected_generation, 0, 0, "expected_generation"),
    expected_summary: requireSummary(scenario.expected_summary, "expected_summary"),
  };
  return Object.freeze(normalized);
}


function requireInput(value) {
  const input = requireExactFields(
    value,
    ROOT_FIELDS,
    [
      "schema_version",
      "session",
      "csrf",
      "public_origin",
      "principal_id",
      "guild_id",
      "installation_id",
      "run_id",
      "manifest_sha256",
      "operation",
    ],
    "input",
  );
  if (input.schema_version !== 1) {
    fail("schema_version_invalid");
  }
  const session = requireSecret(input.session, "session");
  const csrf = requireSecret(input.csrf, "csrf");
  if (session === csrf) {
    fail("credentials_not_distinct");
  }
  let origin;
  try {
    const parsed = new URL(input.public_origin);
    if (
      parsed.origin !== D2_ORIGIN ||
      parsed.href !== `${D2_ORIGIN}/` ||
      parsed.username !== "" ||
      parsed.password !== ""
    ) {
      fail("public_origin_invalid");
    }
    origin = parsed.origin;
  } catch (error) {
    if (error instanceof RunnerFailure) {
      throw error;
    }
    fail("public_origin_invalid");
  }
  const operation = typeof input.operation === "string" ? input.operation : "";
  if (!OPERATIONS.has(operation)) {
    fail("operation_invalid");
  }
  if (
    typeof input.principal_id !== "string" ||
    !input.principal_id.startsWith("discord:") ||
    !SNOWFLAKE.test(input.principal_id.slice("discord:".length))
  ) {
    fail("principal_id_invalid");
  }
  if (operation === "auth-smoke" && Object.hasOwn(input, "scenario")) {
    fail("scenario_not_allowed");
  }
  if (operation === "auth-smoke" && Object.hasOwn(input, "scenario_sha256")) {
    fail("scenario_digest_not_allowed");
  }
  if (operation === "auth-smoke" && Object.hasOwn(input, "authoring_session_id")) {
    fail("authoring_session_id_not_allowed");
  }
  if (operation === "one-shot" && !Object.hasOwn(input, "scenario")) {
    fail("scenario_required");
  }
  if (operation === "one-shot" && !Object.hasOwn(input, "scenario_sha256")) {
    fail("scenario_digest_required");
  }
  if (operation === "one-shot" && !Object.hasOwn(input, "authoring_session_id")) {
    fail("authoring_session_id_required");
  }
  const scenario = operation === "one-shot" ? requireScenario(input.scenario) : null;
  const authoringSessionId = operation === "one-shot"
    ? requireString(input.authoring_session_id, RESOURCE_ID, "authoring_session_id")
    : null;
  if (operation === "one-shot") {
    const expectedPrefix = `${scenario.session_id_prefix}-`;
    const suffix = authoringSessionId.slice(expectedPrefix.length);
    if (!authoringSessionId.startsWith(expectedPrefix) || !/^[0-9a-f]{16}$/.test(suffix)) {
      fail("authoring_session_id_binding_invalid");
    }
  }
  return Object.freeze({
    schema_version: 1,
    session,
    csrf,
    public_origin: origin,
    principal_id: input.principal_id,
    guild_id: requireString(input.guild_id, SNOWFLAKE, "guild_id"),
    installation_id: requireString(input.installation_id, RESOURCE_ID, "installation_id"),
    run_id: requireString(input.run_id, RUN_ID, "run_id"),
    manifest_sha256: requireString(input.manifest_sha256, DIGEST, "manifest_sha256"),
    operation,
    scenario,
    scenario_sha256: operation === "one-shot"
      ? requireString(input.scenario_sha256, DIGEST, "scenario_sha256")
      : null,
    authoring_session_id: authoringSessionId,
  });
}


function canonicalJson(value) {
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return JSON.stringify(value);
  }
  if (typeof value === "number" && Number.isSafeInteger(value)) {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map((item) => canonicalJson(item)).join(",")}]`;
  }
  if (isObject(value)) {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  fail("canonical_json_invalid");
}


function requireObservedAt(value) {
  if (typeof value !== "string" || !UTC_TIMESTAMP.test(value) || Number.isNaN(Date.parse(value))) {
    fail("clock_invalid");
  }
  return value;
}


function makeCredentialedFetch(input, fetchImpl) {
  if (typeof fetchImpl !== "function") {
    fail("fetch_unavailable");
  }
  const cookie = `__Host-starring_session=${input.session}; __Host-starring_csrf=${input.csrf}`;
  return async (resource, options = {}) => {
    let url;
    try {
      url = new URL(resource);
    } catch {
      fail("request_url_invalid");
    }
    if (
      url.origin !== input.public_origin ||
      (!url.pathname.startsWith("/v1/") && !url.pathname.startsWith("/v2/")) ||
      url.username !== "" ||
      url.password !== ""
    ) {
      fail("request_scope_invalid");
    }
    if (options.redirect !== "error") {
      fail("redirect_policy_invalid");
    }
    const method = String(options.method || "GET").toUpperCase();
    const headers = new Headers(options.headers || {});
    headers.set("cookie", cookie);
    if (method !== "GET") {
      headers.set("origin", input.public_origin);
      if (headers.get("x-csrf-token") !== input.csrf) {
        fail("csrf_header_invalid");
      }
    } else {
      headers.delete("origin");
    }
    let response;
    try {
      response = await fetchImpl(url.href, {
        ...options,
        method,
        headers,
        redirect: "error",
      });
    } catch {
      fail("network_request_failed");
    }
    if (
      !response ||
      !Number.isInteger(response.status) ||
      response.redirected === true ||
      (response.status >= 300 && response.status < 400)
    ) {
      fail("redirect_or_response_invalid");
    }
    return response;
  };
}


async function loadDriver(input, fetchImpl, dependencies) {
  let source;
  try {
    source = dependencies.productDriverSource === undefined
      ? await readFile(PRODUCT_DRIVER_URL, "utf8")
      : dependencies.productDriverSource;
  } catch {
    fail("product_driver_unavailable");
  }
  if (typeof source !== "string" || source.length < 1 || Buffer.byteLength(source, "utf8") > 1024 * 1024) {
    fail("product_driver_invalid");
  }
  const context = {
    URL,
    Error,
    Object,
    Promise,
    AbortController,
    setTimeout,
    clearTimeout,
    crypto: webcrypto,
    TextEncoder,
    globalThis: null,
  };
  context.globalThis = context;
  try {
    vm.runInNewContext(source, context, {
      filename: "product_driver.js",
      timeout: 1000,
    });
  } catch {
    fail("product_driver_invalid");
  }
  if (!context.StarringD2ProductDriver || typeof context.StarringD2ProductDriver.create !== "function") {
    fail("product_driver_invalid");
  }
  let driver;
  try {
    driver = context.StarringD2ProductDriver.create({
      origin: input.public_origin,
      fetchImpl,
      cookieSource: () => `__Host-starring_csrf=${input.csrf}`,
      randomUUID: dependencies.randomUUID || randomUUID,
      now: dependencies.now || (() => new Date().toISOString()),
      sleep: dependencies.sleep,
      requestTimeoutMilliseconds: dependencies.requestTimeoutMilliseconds,
    });
  } catch {
    fail("product_driver_initialization_failed");
  }
  return driver;
}


async function requireIdentityAndAuthority(driver, input) {
  const identity = await driver.me();
  if (
    identity.status !== 200 ||
    !isObject(identity.body) ||
    identity.body.principal_id !== input.principal_id
  ) {
    fail("principal_binding_mismatch");
  }
  const authority = await driver.authorityCheck(input.installation_id);
  if (authority.status !== 204 || authority.body !== null) {
    fail("authority_check_failed");
  }
  return Object.freeze({ me: identity.status, authority_check: authority.status });
}


function matchesConfirmation(actual, input, scenario) {
  if (!isObject(actual) || actual.installation_id !== input.installation_id) {
    fail("scenario_confirmation_mismatch");
  }
  if (
    scenario.expected_summary &&
    canonicalJson(actual.summary) !== canonicalJson(scenario.expected_summary)
  ) {
    fail("scenario_confirmation_mismatch");
  }
  return true;
}


async function runOperation(driver, input) {
  const authStatuses = await requireIdentityAndAuthority(driver, input);
  if (input.operation === "auth-smoke") {
    return Object.freeze({ authStatuses });
  }
  const scenario = input.scenario;
  const flow = await driver.runOneShotProductFlow({
    installationId: input.installation_id,
    sessionId: input.authoring_session_id,
    expectedGeneration: scenario.expected_generation,
    message: scenario.message,
    confirmPreview: async (actual) => matchesConfirmation(actual, input, scenario),
  });
  const live = await driver.waitForLive({
    installationId: input.installation_id,
    promotionId: flow.promotion.promotion_id,
    pendingObserved: flow.runtime_pending_observed,
  });
  return Object.freeze({ authStatuses, flow, live });
}


async function consumeBoundedResponse(response) {
  if (typeof response.text !== "function") {
    fail("response_body_invalid");
  }
  let body;
  try {
    body = await response.text();
  } catch {
    fail("response_body_invalid");
  }
  if (typeof body !== "string" || Buffer.byteLength(body, "utf8") > MAX_RESPONSE_BYTES) {
    fail("response_body_invalid");
  }
  return body;
}


async function verifyLogout(fetchImpl, input) {
  const logout = await fetchImpl(`${input.public_origin}/v1/logout`, {
    method: "POST",
    headers: {
      accept: "application/json",
      "x-csrf-token": input.csrf,
    },
    body: "",
    credentials: "same-origin",
    cache: "no-store",
    redirect: "error",
  });
  await consumeBoundedResponse(logout);
  if (logout.status !== 204) {
    fail("logout_status_invalid");
  }
  const after = await fetchImpl(`${input.public_origin}/v1/me`, {
    method: "GET",
    headers: { accept: "application/json" },
    credentials: "same-origin",
    cache: "no-store",
    redirect: "error",
  });
  await consumeBoundedResponse(after);
  if (after.status !== 401) {
    fail("post_logout_session_active");
  }
  return Object.freeze({ logout: 204, post_logout_me: 401 });
}


function sanitizeFailure(error, fallback = "operation_failed") {
  if (error instanceof RunnerFailure) {
    return error;
  }
  if (
    error &&
    error.name === "StarringD2ProductRequestError" &&
    Number.isInteger(error.status)
  ) {
    const productCode = typeof error.code === "string" && /^[a-z][a-z0-9_]{0,63}$/.test(error.code)
      ? error.code
      : "response_invalid";
    return new RunnerFailure(`product_request_${error.status}_${productCode}`);
  }
  const stableMessages = new Set([
    "authoring_not_preview_ready",
    "promotion_not_pending_approval",
    "approval_preview_not_pending",
    "promotion_preview_payload_mismatch",
    "preview_not_approved_by_operator",
    "promotion_not_approved",
    "promotion_not_applied",
    "authoring_http_status_invalid",
    "apply_resolution_timeout",
    "runtime_drain_attempts_exhausted",
    "deployment_failed",
    "deployment_live_timeout",
    "live_projection_identity_invalid",
  ]);
  if (error && typeof error.message === "string" && stableMessages.has(error.message)) {
    return new RunnerFailure(error.message);
  }
  return new RunnerFailure(fallback);
}


function buildEvidence(input, operationResult, logoutStatuses, observedAt) {
  const common = {
    schema_version: 1,
    kind: input.operation === "auth-smoke"
      ? "starring.d2a.authentication-evidence.v1"
      : "starring.d2a.one-shot-product-evidence.v1",
    observed_at: observedAt,
    certification_class: "automated_maintenance_v1",
    operation: input.operation,
    public_origin: input.public_origin,
    principal_id: input.principal_id,
    guild_id: input.guild_id,
    installation_id: input.installation_id,
    run_id: input.run_id,
    manifest_sha256: input.manifest_sha256,
    direct_auth_used: true,
    release_eligible: false,
    uncovered_release_boundaries: UNCOVERED_RELEASE_BOUNDARIES,
  };
  const authStatuses = operationResult.authStatuses;
  if (input.operation === "auth-smoke") {
    return Object.freeze({
      ...common,
      me_status: authStatuses.me,
      authority_check_status: authStatuses.authority_check,
      logout_status: logoutStatuses.logout,
      post_logout_me_status: logoutStatuses.post_logout_me,
    });
  }
  const flow = operationResult.flow;
  const candidateRulesetHash = requireString(
    flow.preview && flow.preview.candidate_ruleset_hash,
    DIGEST,
    "result_candidate_ruleset_hash",
  );
  const targetContentHash = requireString(
    flow.preview && flow.preview.target_content_hash,
    DIGEST,
    "result_target_content_hash",
  );
  const payloadDigest = requireString(
    flow.preview && flow.preview.payload_digest,
    DIGEST,
    "result_payload_digest",
  );
  const promotionId = requireString(
    flow.promotion && flow.promotion.promotion_id,
    DIGEST,
    "result_promotion_id",
  );
  const authoringSessionId = requireString(
    flow.authoring && flow.authoring.session_id,
    RESOURCE_ID,
    "result_authoring_session_id",
  );
  if (authoringSessionId !== input.authoring_session_id) {
    fail("result_authoring_session_binding_invalid");
  }
  const live = operationResult.live;
  if (
    !isObject(live) ||
    live.schema_version !== 1 ||
    live.kind !== "starring.d2.browser-live-evidence.v1" ||
    live.installation_id !== input.installation_id ||
    live.promotion_id !== promotionId ||
    live.public_origin !== input.public_origin ||
    live.live_observed !== true
  ) {
    fail("live_evidence_binding_invalid");
  }
  return Object.freeze({
    ...common,
    scenario_sha256: input.scenario_sha256,
    logout_status: logoutStatuses.logout,
    post_logout_me_status: logoutStatuses.post_logout_me,
    authoring_http_status: requireInteger(flow.authoring_http_status, 200, 299, "authoring_status"),
    promotion_http_status: requireInteger(flow.promotion_http_status, 200, 299, "promotion_status"),
    preview_http_status: requireInteger(flow.preview_http_status, 200, 299, "preview_status"),
    approval_http_status: requireInteger(flow.approval_http_status, 200, 299, "approval_status"),
    apply_http_status: requireInteger(flow.apply_http_status, 200, 299, "apply_status"),
    authoring_session_id: authoringSessionId,
    authoring_generation: requireInteger(
      flow.authoring && flow.authoring.generation,
      1,
      Number.MAX_SAFE_INTEGER,
      "result_authoring_generation",
    ),
    promotion_id: promotionId,
    candidate_ruleset_hash: candidateRulesetHash,
    target_content_hash: targetContentHash,
    payload_digest: payloadDigest,
    summary: requireSummary(flow.preview && flow.preview.summary, "result_summary"),
    preview_state: requireChoice(
      flow.preview && flow.preview.state,
      new Set(["pending_approval"]),
      "preview_state",
    ),
    approval_state: requireChoice(
      flow.approval && flow.approval.state,
      new Set(["approved"]),
      "approval_state",
    ),
    apply_state: requireChoice(
      flow.applied && flow.applied.state,
      new Set(["runtime_pending", "live"]),
      "apply_state",
    ),
    apply_attempts: requireInteger(flow.apply_attempts, 1, 180, "apply_attempts"),
    runtime_drain_observed: requireBoolean(
      flow.runtime_drain_observed,
      "runtime_drain_observed",
    ),
    runtime_pending_observed: requireBoolean(
      flow.runtime_pending_observed,
      "runtime_pending_observed",
    ),
    apply_resumed_after_conflict: requireBoolean(
      flow.apply_resumed_after_conflict,
      "apply_resumed_after_conflict",
    ),
    apply_status_observations: requireInteger(
      flow.apply_status_observations,
      0,
      180,
      "apply_status_observations",
    ),
    live_observed_at: requireObservedAt(live.observed_at),
    deployment_http_status: requireInteger(
      live.deployment_http_status,
      200,
      200,
      "deployment_http_status",
    ),
    operational_http_status: requireInteger(
      live.operational_http_status,
      200,
      200,
      "operational_http_status",
    ),
    live_attempts: requireInteger(live.attempts, 1, 180, "live_attempts"),
    pending_observed: requireBoolean(live.pending_observed, "pending_observed"),
    live_observed: true,
    product_state: requireChoice(live.product_state, new Set(["live"]), "product_state"),
    operational_state: requireChoice(
      live.operational_state,
      new Set(["live"]),
      "operational_state",
    ),
    runtime_phase: requireChoice(live.runtime_phase, new Set(["live"]), "runtime_phase"),
    serving_state: requireChoice(live.serving_state, new Set(["fresh"]), "serving_state"),
    deployment_observed_at: requireObservedAt(live.deployment_observed_at),
    deployment_attestation_revision: requireInteger(
      live.deployment_attestation_revision,
      1,
      Number.MAX_SAFE_INTEGER,
      "deployment_attestation_revision",
    ),
    deployment_last_heartbeat_at: requireObservedAt(live.deployment_last_heartbeat_at),
    deployment_lease_expires_at: requireObservedAt(live.deployment_lease_expires_at),
    decision_observed_at: requireObservedAt(live.decision_observed_at),
    runtime_observed_at: requireObservedAt(live.runtime_observed_at),
    current_attempt: requireInteger(
      live.current_attempt,
      1,
      Number.MAX_SAFE_INTEGER,
      "current_attempt",
    ),
    attestation_revision: requireInteger(
      live.attestation_revision,
      1,
      Number.MAX_SAFE_INTEGER,
      "attestation_revision",
    ),
    convergence_attempt: requireInteger(
      live.convergence_attempt,
      1,
      Number.MAX_SAFE_INTEGER,
      "convergence_attempt",
    ),
    process_instance_id: requireString(
      live.process_instance_id,
      PROCESS_INSTANCE_ID,
      "process_instance_id",
    ),
    last_heartbeat_at: requireObservedAt(live.last_heartbeat_at),
    lease_expires_at: requireObservedAt(live.lease_expires_at),
  });
}


export async function executeHeadless(rawInput, dependencies = {}) {
  const input = requireInput(rawInput);
  const baseFetch = dependencies.fetchImpl || globalThis.fetch;
  const credentialedFetch = makeCredentialedFetch(input, baseFetch);
  let operationResult;
  let operationError = null;
  try {
    const driver = await loadDriver(input, credentialedFetch, dependencies);
    operationResult = await runOperation(driver, input);
  } catch (error) {
    operationError = error;
  }

  let logoutStatuses;
  try {
    logoutStatuses = await verifyLogout(credentialedFetch, input);
  } catch (error) {
    throw sanitizeFailure(error, "logout_verification_failed");
  }
  if (operationError) {
    throw sanitizeFailure(operationError);
  }
  const observedAt = requireObservedAt(
    (dependencies.now || (() => new Date().toISOString()))(),
  );
  return buildEvidence(
    input,
    operationResult,
    logoutStatuses,
    observedAt,
  );
}


async function readBoundedJson(stream) {
  const chunks = [];
  let size = 0;
  for await (const chunk of stream) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += bytes.length;
    if (size > MAX_INPUT_BYTES) {
      fail("input_too_large");
    }
    chunks.push(bytes);
  }
  if (size === 0) {
    fail("input_empty");
  }
  let parsed;
  try {
    parsed = JSON.parse(Buffer.concat(chunks, size).toString("utf8"));
  } catch {
    fail("input_json_invalid");
  }
  if (!isObject(parsed)) {
    fail("input_invalid");
  }
  return parsed;
}


function serialized(value) {
  const output = `${canonicalJson(value)}\n`;
  if (Buffer.byteLength(output, "utf8") > MAX_OUTPUT_BYTES) {
    fail("output_too_large");
  }
  return output;
}


export async function runCli({ stdin = process.stdin, stdout = process.stdout, dependencies = {} } = {}) {
  try {
    const evidence = await executeHeadless(await readBoundedJson(stdin), dependencies);
    stdout.write(serialized(evidence));
    return 0;
  } catch (error) {
    const failure = sanitizeFailure(error, "runner_failed");
    stdout.write(serialized({
      schema_version: 1,
      kind: "starring.d2a.runner-error.v1",
      ok: false,
      error_code: failure.code,
    }));
    return 1;
  }
}


if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await runCli();
}
