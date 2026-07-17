import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import {
  DISABLED_CODEX_FEATURES,
  buildTrustedPrompt,
  codexArguments,
  createCodexRunner,
  isChatGptLoginStatus,
  isSupportedCodexVersion,
} from "./codex-runner.mjs";
import {
  AUTH_MODE,
  CODEX_CLI_VERSION,
  MODEL,
  PROVIDER,
  REASONING_EFFORT,
} from "./protocol.mjs";
import { startWorker, workerSourceSha256 } from "./worker.mjs";

const TOKEN = "test-worker-token-1234567890";
const VERSION = CODEX_CLI_VERSION;

test("worker source identity is stable and exact", async () => {
  const first = await workerSourceSha256();
  const second = await workerSourceSha256();
  assert.match(first, /^[0-9a-f]{64}$/);
  assert.equal(first, second);
});

function verification(overrides = {}) {
  return {
    codex_cli_version: VERSION,
    auth_mode: AUTH_MODE,
    ...overrides,
  };
}

function runnerResult(overrides = {}) {
  return {
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    auth_mode: AUTH_MODE,
    codex_cli_version: VERSION,
    arguments: JSON.stringify({ accepted: true }),
    usage: {
      input_tokens: 100,
      cached_input_tokens: 80,
      output_tokens: 20,
      reasoning_output_tokens: 10,
    },
    ...overrides,
  };
}

function fakeRunner(options = {}) {
  return {
    verify: options.verify ?? (async () => verification()),
    complete: options.complete ?? (async () => runnerResult()),
  };
}

function captureIdentities(observations) {
  let invocation = 0;
  return async (_command, args) => {
    const index = Math.min(Math.floor(invocation / 2), observations.length - 1);
    const observation = observations[index];
    invocation += 1;
    if (args[0] === "--version") {
      return {
        stdout: observation.version ?? VERSION,
        stderr: "",
      };
    }
    return {
      stdout: "",
      stderr: observation.login ?? "Logged in using ChatGPT",
    };
  };
}

function completionRequest(overrides = {}) {
  return {
    schema_version: 1,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    messages: [
      { role: "system", content: "Return structured intent." },
      { role: "user", content: "Build a private room." },
    ],
    frontier: {
      name: "interpret_intent_core",
      description: "Interpret the bounded intent core.",
      parameters: {
        type: "object",
        properties: {
          accepted: { type: "boolean" },
        },
        required: ["accepted"],
        additionalProperties: false,
      },
    },
    ...overrides,
  };
}

function sortedKeys(value) {
  return Object.keys(value).sort();
}

async function responseJson(base, path, options = {}) {
  const headers = {};
  if (options.token !== null) {
    headers.authorization = `Bearer ${options.token ?? TOKEN}`;
  }
  if (options.body !== undefined) {
    headers["content-type"] = "application/json";
  }
  const response = await fetch(`${base}${path}`, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });
  return {
    status: response.status,
    body: await response.json(),
  };
}

async function createFixture(options = {}) {
  const directory = await mkdtemp(join(tmpdir(), "starring-codex-worker-test-"));
  const metricsPath = join(directory, "metrics", "worker.jsonl");
  const worker = await startWorker({
    token: TOKEN,
    port: 0,
    runner: options.runner ?? fakeRunner(),
    metricsPath,
    tempRoot: join(directory, "requests"),
    concurrency: options.concurrency,
    maxQueue: options.maxQueue,
    timeoutMs: options.timeoutMs,
    metricsMaxBytes: options.metricsMaxBytes,
    metricsBackups: options.metricsBackups,
    instanceId: options.instanceId ?? "test-worker-instance",
    workerSourceSha256: options.workerSourceSha256 ?? "a".repeat(64),
    timelineClock: options.timelineClock,
  });
  const protocol = "http:";
  const base = `${protocol}/${"/"}${worker.address.address}:${worker.address.port}`;
  return {
    base,
    directory,
    metricsPath,
    worker,
    async cleanup() {
      await worker.close();
      await rm(directory, { recursive: true, force: true });
    },
  };
}

async function waitFor(predicate, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error("condition_timeout");
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 5));
  }
}

async function waitForMetricsHealth(fixture, predicate, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs;
  while (true) {
    const response = await responseJson(fixture.base, "/metrics-health");
    if (response.status === 200 && predicate(response.body)) {
      return response.body;
    }
    if (Date.now() >= deadline) {
      throw new Error("metrics_health_timeout");
    }
    await delay(5);
  }
}

function deferred() {
  let resolvePromise;
  const promise = new Promise((resolveValue) => {
    resolvePromise = resolveValue;
  });
  return { promise, resolve: resolvePromise };
}

function delay(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

async function metricRecords(fixture) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    await fixture.worker.metrics.flush();
    try {
      const log = (await readFile(fixture.metricsPath, "utf8")).trim();
      return log.length === 0 ? [] : log.split("\n").map((line) => JSON.parse(line));
    } catch (error) {
      if (error?.code !== "ENOENT") {
        throw error;
      }
    }
    await delay(5);
  }
  throw new Error("metrics_not_written");
}

function completionFetch(base, signal) {
  return fetch(`${base}/v1/frontier-completions`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${TOKEN}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(completionRequest()),
    signal,
  });
}

test("health is authenticated, exact, and loopback only", async () => {
  await assert.rejects(
    startWorker({
      host: "0.0.0.0",
      token: TOKEN,
      port: 0,
      runner: fakeRunner(),
    }),
    (error) => error.code === "loopback_only",
  );
  const fixture = await createFixture();
  try {
    assert.equal(fixture.worker.address.address, "127.0.0.1");
    const missing = await responseJson(fixture.base, "/health", { token: null });
    assert.equal(missing.status, 401);
    assert.deepEqual(missing.body, { error: { code: "unauthorized" } });
    const missingMetrics = await responseJson(fixture.base, "/metrics-health", { token: null });
    assert.equal(missingMetrics.status, 401);
    assert.deepEqual(missingMetrics.body, { error: { code: "unauthorized" } });

    const health = await responseJson(fixture.base, "/health");
    assert.equal(health.status, 200);
    assert.deepEqual(sortedKeys(health.body), [
      "accepted_requests_total",
      "active_requests",
      "auth_mode",
      "codex_cli_version",
      "concurrency_limit",
      "instance_id",
      "model",
      "provider",
      "queue_capacity",
      "queued_requests",
      "reasoning_effort",
      "request_timeout_ms",
      "schema_version",
      "settled_requests_total",
      "status",
      "worker_source_sha256",
    ]);
    assert.deepEqual(health.body, {
      schema_version: 1,
      status: "ok",
      provider: PROVIDER,
      model: MODEL,
      reasoning_effort: REASONING_EFFORT,
      auth_mode: AUTH_MODE,
      codex_cli_version: VERSION,
      instance_id: "test-worker-instance",
      worker_source_sha256: "a".repeat(64),
      concurrency_limit: 2,
      queue_capacity: 8,
      request_timeout_ms: 55_000,
      active_requests: 0,
      queued_requests: 0,
      accepted_requests_total: 0,
      settled_requests_total: 0,
    });
    const metricsHealth = await responseJson(fixture.base, "/metrics-health");
    assert.equal(metricsHealth.status, 200);
    assert.deepEqual(metricsHealth.body, {
      schema_version: 1,
      instance_id: "test-worker-instance",
      worker_source_sha256: "a".repeat(64),
      status: "ok",
      writable_verified: true,
      records_attempted: 0,
      records_written: 0,
      pending_records: 0,
      write_failures_total: 0,
      last_error_code: null,
    });
  } finally {
    await fixture.cleanup();
  }
});

test("request identity and sole frontier shape fail closed", async () => {
  const fixture = await createFixture();
  try {
    const wrongModel = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest({ model: "wrong-model" }),
    });
    assert.equal(wrongModel.status, 400);
    assert.equal(wrongModel.body.error.code, "identity_mismatch");

    const wrongEffort = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest({ reasoning_effort: "low" }),
    });
    assert.equal(wrongEffort.status, 400);
    assert.equal(wrongEffort.body.error.code, "identity_mismatch");

    const extra = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: { ...completionRequest(), extra: true },
    });
    assert.equal(extra.status, 400);
    assert.equal(extra.body.error.code, "invalid_request_shape");

    const manyFrontiers = completionRequest();
    manyFrontiers.frontier = [manyFrontiers.frontier, manyFrontiers.frontier];
    const multiple = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: manyFrontiers,
    });
    assert.equal(multiple.status, 400);
    assert.equal(multiple.body.error.code, "invalid_frontier");

    const extraFrontierField = completionRequest();
    extraFrontierField.frontier.extra = true;
    const extraFrontier = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: extraFrontierField,
    });
    assert.equal(extraFrontier.status, 400);
    assert.equal(extraFrontier.body.error.code, "invalid_frontier");
    const health = await responseJson(fixture.base, "/health");
    assert.equal(health.body.accepted_requests_total, 0);
    assert.equal(health.body.settled_requests_total, 0);
  } finally {
    await fixture.cleanup();
  }
});

test("successful completion returns the exact native envelope", async () => {
  const calls = [];
  const ticks = [100, 105, 120, 122].map((value) => BigInt(value) * 1_000_000n);
  const fixture = await createFixture({
    timelineClock: () => ticks.shift(),
    runner: fakeRunner({
      complete: async (input) => {
        calls.push(input);
        return runnerResult({
          arguments: JSON.stringify({ accepted: true, locale: "ko" }),
        });
      },
    }),
  });
  try {
    const result = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(result.status, 200);
    assert.deepEqual(sortedKeys(result.body), [
      "auth_mode",
      "codex_cli_version",
      "duration_ms",
      "model",
      "provider",
      "reasoning_effort",
      "request_id",
      "schema_version",
      "tool_call",
      "usage",
    ]);
    assert.deepEqual(sortedKeys(result.body.tool_call), ["arguments", "id", "name"]);
    assert.deepEqual(sortedKeys(result.body.usage), [
      "cached_input_tokens",
      "input_tokens",
      "output_tokens",
      "reasoning_output_tokens",
    ]);
    assert.equal(result.body.schema_version, 1);
    assert.equal(result.body.provider, PROVIDER);
    assert.equal(result.body.model, MODEL);
    assert.equal(result.body.reasoning_effort, REASONING_EFFORT);
    assert.equal(result.body.auth_mode, AUTH_MODE);
    assert.equal(result.body.codex_cli_version, VERSION);
    assert.equal(result.body.tool_call.name, "interpret_intent_core");
    assert.deepEqual(JSON.parse(result.body.tool_call.arguments), {
      accepted: true,
      locale: "ko",
    });
    assert.match(result.body.tool_call.id, /^call-[0-9a-f-]+$/);
    assert.ok(Number.isSafeInteger(result.body.duration_ms));
    assert.equal(calls.length, 1);
    assert.equal(calls[0].model, MODEL);
    assert.equal(calls[0].reasoningEffort, REASONING_EFFORT);
    assert.deepEqual(calls[0].messages, completionRequest().messages);
    const health = await responseJson(fixture.base, "/health");
    assert.equal(health.body.accepted_requests_total, 1);
    assert.equal(health.body.settled_requests_total, 1);
    assert.equal(health.body.active_requests, 0);
    assert.equal(health.body.queued_requests, 0);
    const records = await metricRecords(fixture);
    assert.equal(records.length, 1);
    assert.deepEqual(sortedKeys(records[0]), [
      "active_at_admission",
      "concurrency_limit",
      "duration_ms",
      "error_code",
      "frontier_name",
      "instance_id",
      "metric_schema_version",
      "model",
      "outcome",
      "post_runner_ms",
      "provider",
      "queue_capacity",
      "queue_wait_ms",
      "queued_at_admission",
      "reasoning_effort",
      "request_id",
      "request_timeout_ms",
      "result_validation_started",
      "runner_duration_ms",
      "runner_elapsed_at_terminal_ms",
      "runner_outcome",
      "runner_settled",
      "runner_started",
      "status_code",
      "terminal_stage",
      "timestamp",
      "total_duration_ms",
      "usage",
      "worker_source_sha256",
    ]);
    assert.equal(records[0].metric_schema_version, 2);
    assert.equal(records[0].request_id, result.body.request_id);
    assert.equal(records[0].instance_id, "test-worker-instance");
    assert.equal(records[0].worker_source_sha256, "a".repeat(64));
    assert.equal(records[0].concurrency_limit, 2);
    assert.equal(records[0].queue_capacity, 8);
    assert.equal(records[0].request_timeout_ms, 55_000);
    assert.equal(records[0].active_at_admission, 0);
    assert.equal(records[0].queued_at_admission, 0);
    assert.equal(records[0].queue_wait_ms, 5);
    assert.equal(records[0].runner_duration_ms, 15);
    assert.equal(records[0].runner_elapsed_at_terminal_ms, null);
    assert.equal(records[0].post_runner_ms, 2);
    assert.equal(records[0].total_duration_ms, 22);
    assert.equal(records[0].runner_started, true);
    assert.equal(records[0].runner_settled, true);
    assert.equal(records[0].runner_outcome, "resolved");
    assert.equal(records[0].result_validation_started, true);
    assert.equal(records[0].terminal_stage, "completed");
    assert.equal(records[0].duration_ms, result.body.duration_ms);
    assert.deepEqual(records[0].usage, result.body.usage);
    const metricsHealth = await waitForMetricsHealth(
      fixture,
      (body) => body.records_written === 1 && body.pending_records === 0,
    );
    assert.equal(metricsHealth.status, "ok");
    assert.equal(metricsHealth.records_attempted, 1);
    assert.equal(metricsHealth.write_failures_total, 0);
    assert.equal((await stat(dirname(fixture.metricsPath))).mode & 0o777, 0o700);
    assert.equal((await stat(fixture.metricsPath)).mode & 0o777, 0o600);
  } finally {
    await fixture.cleanup();
  }
});

test("runner identity mismatch is never relabeled as Luna", async () => {
  const fixture = await createFixture({
    runner: fakeRunner({
      complete: async () => runnerResult({ model: "other-model" }),
    }),
  });
  try {
    const result = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(result.status, 502);
    assert.deepEqual(result.body, { error: { code: "provider_identity_mismatch" } });
    const records = await metricRecords(fixture);
    assert.equal(records.length, 1);
    assert.equal(records[0].terminal_stage, "result_validation");
    assert.equal(records[0].runner_started, true);
    assert.equal(records[0].runner_settled, true);
    assert.equal(records[0].runner_outcome, "resolved");
    assert.equal(records[0].result_validation_started, true);
    assert.equal(records[0].usage, null);
  } finally {
    await fixture.cleanup();
  }
});

test("bounded queue rejects overflow without retrying", async () => {
  const gate = deferred();
  let calls = 0;
  const fixture = await createFixture({
    concurrency: 2,
    maxQueue: 1,
    runner: fakeRunner({
      complete: async () => {
        calls += 1;
        await gate.promise;
        return runnerResult();
      },
    }),
  });
  try {
    const first = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    const second = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().active === 2);
    const activeHealth = await responseJson(fixture.base, "/health");
    assert.equal(activeHealth.body.accepted_requests_total, 2);
    assert.equal(activeHealth.body.settled_requests_total, 0);
    assert.equal(activeHealth.body.active_requests, 2);
    assert.equal(activeHealth.body.queued_requests, 0);
    const third = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().queued === 1);
    const queuedHealth = await responseJson(fixture.base, "/health");
    assert.equal(queuedHealth.body.accepted_requests_total, 3);
    assert.equal(queuedHealth.body.settled_requests_total, 0);
    assert.equal(queuedHealth.body.active_requests, 2);
    assert.equal(queuedHealth.body.queued_requests, 1);
    const overflow = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(overflow.status, 429);
    assert.deepEqual(overflow.body, { error: { code: "queue_full" } });
    assert.equal(calls, 2);
    const overflowRecords = await metricRecords(fixture);
    assert.equal(overflowRecords.length, 1);
    assert.equal(overflowRecords[0].error_code, "queue_full");
    assert.equal(overflowRecords[0].terminal_stage, "admission");
    assert.equal(overflowRecords[0].active_at_admission, 2);
    assert.equal(overflowRecords[0].queued_at_admission, 1);
    assert.equal(overflowRecords[0].queue_wait_ms, 0);
    assert.equal(overflowRecords[0].runner_started, false);
    assert.equal(overflowRecords[0].runner_duration_ms, null);
    const rejectedHealth = await responseJson(fixture.base, "/health");
    assert.equal(rejectedHealth.body.accepted_requests_total, 4);
    assert.equal(rejectedHealth.body.settled_requests_total, 1);
    assert.equal(rejectedHealth.body.active_requests, 2);
    assert.equal(rejectedHealth.body.queued_requests, 1);
    gate.resolve();
    const accepted = await Promise.all([first, second, third]);
    assert.deepEqual(accepted.map((entry) => entry.status), [200, 200, 200]);
    assert.equal(calls, 3);
    const settledHealth = await responseJson(fixture.base, "/health");
    assert.equal(settledHealth.body.accepted_requests_total, 4);
    assert.equal(settledHealth.body.settled_requests_total, 4);
    assert.equal(settledHealth.body.active_requests, 0);
    assert.equal(settledHealth.body.queued_requests, 0);
  } finally {
    gate.resolve();
    await fixture.cleanup();
  }
});

test("queued deadline removes stale work without calling the runner", async () => {
  const gate = deferred();
  let calls = 0;
  const fixture = await createFixture({
    concurrency: 1,
    maxQueue: 1,
    timeoutMs: 200,
    runner: fakeRunner({
      complete: async () => {
        calls += 1;
        await gate.promise;
        return runnerResult();
      },
    }),
  });
  try {
    const first = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().active === 1);
    const stale = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().queued === 1);
    const expired = await stale;
    assert.equal(expired.status, 504);
    assert.deepEqual(expired.body, { error: { code: "codex_timeout" } });
    assert.equal(calls, 1);
    const expiredRecords = await metricRecords(fixture);
    assert.equal(expiredRecords.length, 1);
    assert.equal(expiredRecords[0].terminal_stage, "queue");
    assert.equal(expiredRecords[0].runner_started, false);
    assert.equal(expiredRecords[0].runner_duration_ms, null);
    assert.equal(expiredRecords[0].queue_wait_ms, expiredRecords[0].total_duration_ms);
    assert.deepEqual(fixture.worker.stats(), { active: 1, queued: 0 });
    const staleHealth = await responseJson(fixture.base, "/health");
    assert.equal(staleHealth.body.accepted_requests_total, 2);
    assert.equal(staleHealth.body.settled_requests_total, 1);
    assert.equal(staleHealth.body.active_requests, 1);
    assert.equal(staleHealth.body.queued_requests, 0);
    gate.resolve();
    const firstResult = await first;
    assert.equal(firstResult.status, 504);
    const settledHealth = await responseJson(fixture.base, "/health");
    assert.equal(settledHealth.body.accepted_requests_total, 2);
    assert.equal(settledHealth.body.settled_requests_total, 2);
  } finally {
    gate.resolve();
    await fixture.cleanup();
  }
});

test("runner receives only the timeout budget remaining after queue wait", async () => {
  const gate = deferred();
  let calls = 0;
  const fixture = await createFixture({
    concurrency: 1,
    maxQueue: 1,
    timeoutMs: 2_000,
    runner: fakeRunner({
      complete: async ({ signal }) => {
        calls += 1;
        if (calls === 1) {
          await gate.promise;
          return runnerResult();
        }
        return new Promise((resolvePromise, rejectPromise) => {
          const timer = setTimeout(() => resolvePromise(runnerResult()), 1_700);
          const aborted = () => {
            clearTimeout(timer);
            rejectPromise(new Error("aborted"));
          };
          if (signal.aborted) {
            aborted();
          } else {
            signal.addEventListener("abort", aborted, { once: true });
          }
        });
      },
    }),
  });
  try {
    const first = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().active === 1);
    const second = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().queued === 1);
    await delay(600);
    gate.resolve();
    const firstResult = await first;
    const secondResult = await second;
    assert.equal(firstResult.status, 200);
    assert.equal(secondResult.status, 504);
    assert.deepEqual(secondResult.body, { error: { code: "codex_timeout" } });
    assert.equal(calls, 2);
  } finally {
    gate.resolve();
    await fixture.cleanup();
  }
});

test("queued client disconnect removes work without calling the runner", async () => {
  const gate = deferred();
  let calls = 0;
  const fixture = await createFixture({
    concurrency: 1,
    maxQueue: 1,
    timeoutMs: 1_000,
    runner: fakeRunner({
      complete: async () => {
        calls += 1;
        await gate.promise;
        return runnerResult();
      },
    }),
  });
  try {
    const first = completionFetch(fixture.base);
    await waitFor(() => fixture.worker.stats().active === 1);
    const controller = new AbortController();
    const queued = completionFetch(fixture.base, controller.signal);
    await waitFor(() => fixture.worker.stats().queued === 1);
    controller.abort();
    await assert.rejects(queued);
    await waitFor(() => fixture.worker.stats().queued === 0);
    assert.equal(calls, 1);
    const records = await metricRecords(fixture);
    assert.equal(records.length, 1);
    assert.equal(records[0].status_code, 499);
    assert.equal(records[0].error_code, "client_disconnected");
    assert.equal(records[0].terminal_stage, "queue");
    assert.equal(records[0].runner_started, false);
    assert.equal(records[0].queue_wait_ms, records[0].total_duration_ms);
    const disconnectedHealth = await responseJson(fixture.base, "/health");
    assert.equal(disconnectedHealth.body.accepted_requests_total, 2);
    assert.equal(disconnectedHealth.body.settled_requests_total, 1);
    assert.equal(disconnectedHealth.body.active_requests, 1);
    assert.equal(disconnectedHealth.body.queued_requests, 0);
    gate.resolve();
    assert.equal((await first).status, 200);
  } finally {
    gate.resolve();
    await fixture.cleanup();
  }
});

test("active client disconnect aborts the runner and leaves the worker responsive", async () => {
  const started = deferred();
  let aborts = 0;
  const fixture = await createFixture({
    concurrency: 1,
    timeoutMs: 1_000,
    runner: fakeRunner({
      complete: async ({ signal }) => new Promise((resolvePromise, rejectPromise) => {
        const aborted = () => {
          aborts += 1;
          rejectPromise(new Error("aborted"));
        };
        if (signal.aborted) {
          aborted();
          return;
        }
        signal.addEventListener("abort", aborted, { once: true });
        started.resolve();
      }),
    }),
  });
  try {
    const controller = new AbortController();
    const active = completionFetch(fixture.base, controller.signal);
    await started.promise;
    controller.abort();
    await assert.rejects(active);
    await waitFor(() => aborts === 1 && fixture.worker.stats().active === 0);
    const records = await metricRecords(fixture);
    assert.equal(records.length, 1);
    assert.equal(records[0].status_code, 499);
    assert.equal(records[0].error_code, "client_disconnected");
    assert.equal(records[0].terminal_stage, "runner");
    assert.equal(records[0].runner_started, true);
    assert.equal(records[0].runner_settled, true);
    const health = await responseJson(fixture.base, "/health");
    assert.equal(health.status, 200);
    assert.equal(health.body.active_requests, 0);
    assert.equal(health.body.queued_requests, 0);
    assert.equal(health.body.accepted_requests_total, 1);
    assert.equal(health.body.settled_requests_total, 1);
  } finally {
    await fixture.cleanup();
  }
});

test("timeout and runner failure map to bounded transport errors", async () => {
  const timed = await createFixture({
    timeoutMs: 25,
    runner: fakeRunner({
      complete: async ({ signal }) => new Promise((resolvePromise, rejectPromise) => {
        signal.addEventListener("abort", () => rejectPromise(new Error("aborted")), { once: true });
      }),
    }),
  });
  try {
    const timeout = await responseJson(timed.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(timeout.status, 504);
    assert.deepEqual(timeout.body, { error: { code: "codex_timeout" } });
    const timedRecords = await metricRecords(timed);
    assert.equal(timedRecords.length, 1);
    assert.equal(timedRecords[0].terminal_stage, "runner");
    assert.equal(timedRecords[0].runner_started, true);
    assert.equal(timedRecords[0].runner_settled, true);
    assert.equal(timedRecords[0].runner_outcome, "rejected");
    const health = await responseJson(timed.base, "/health");
    assert.equal(health.body.accepted_requests_total, 1);
    assert.equal(health.body.settled_requests_total, 1);
    assert.equal(health.body.active_requests, 0);
    assert.equal(health.body.queued_requests, 0);
  } finally {
    await timed.cleanup();
  }

  const failed = await createFixture({
    runner: fakeRunner({
      complete: async () => {
        throw new Error("private runner diagnostic");
      },
    }),
  });
  try {
    const failure = await responseJson(failed.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(failure.status, 502);
    assert.deepEqual(failure.body, { error: { code: "runner_failure" } });
    const failedRecords = await metricRecords(failed);
    assert.equal(failedRecords.length, 1);
    assert.equal(failedRecords[0].terminal_stage, "runner");
    assert.equal(failedRecords[0].runner_outcome, "rejected");
    assert.equal(failedRecords[0].usage, null);
    const health = await responseJson(failed.base, "/health");
    assert.equal(health.body.accepted_requests_total, 1);
    assert.equal(health.body.settled_requests_total, 1);
    assert.equal(health.body.active_requests, 0);
    assert.equal(health.body.queued_requests, 0);
  } finally {
    await failed.cleanup();
  }
});

test("timeout keeps an ignored-abort runner duration censored until counters settle", async () => {
  const gate = deferred();
  let calls = 0;
  const fixture = await createFixture({
    concurrency: 1,
    maxQueue: 0,
    timeoutMs: 25,
    runner: fakeRunner({
      complete: async () => {
        calls += 1;
        await gate.promise;
        return runnerResult();
      },
    }),
  });
  try {
    const result = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(result.status, 504);
    assert.deepEqual(result.body, { error: { code: "codex_timeout" } });
    assert.equal(calls, 1);
    const records = await metricRecords(fixture);
    assert.equal(records.length, 1);
    assert.equal(records[0].terminal_stage, "runner");
    assert.equal(records[0].runner_started, true);
    assert.equal(records[0].runner_settled, false);
    assert.equal(records[0].runner_outcome, null);
    assert.equal(records[0].runner_duration_ms, null);
    assert.ok(records[0].runner_elapsed_at_terminal_ms >= 2_500);
    assert.equal(fixture.worker.stats().active, 1);
    const activeHealth = await responseJson(fixture.base, "/health");
    assert.equal(activeHealth.body.accepted_requests_total, 1);
    assert.equal(activeHealth.body.settled_requests_total, 0);
    gate.resolve();
    await waitFor(() => fixture.worker.stats().active === 0);
    const settledHealth = await responseJson(fixture.base, "/health");
    assert.equal(settledHealth.body.accepted_requests_total, 1);
    assert.equal(settledHealth.body.settled_requests_total, 1);
    assert.equal((await metricRecords(fixture)).length, 1);
  } finally {
    gate.resolve();
    await fixture.cleanup();
  }
});

test("metrics never contain prompts, outputs, bearer tokens, or runner diagnostics", async () => {
  const secret = "TOP_SECRET_PROMPT_VALUE_64931";
  const outputSecret = "TOP_SECRET_OUTPUT_VALUE_82175";
  const runnerSecret = "TOP_SECRET_RUNNER_VALUE_39142";
  const fixture = await createFixture({
    runner: fakeRunner({
      complete: async (input) => {
        if (input.messages[1].content.includes("fail")) {
          throw new Error(runnerSecret);
        }
        return runnerResult({
          arguments: JSON.stringify({ accepted: true, value: outputSecret }),
        });
      },
    }),
  });
  const request = completionRequest();
  request.messages[1].content = secret;
  const failedRequest = completionRequest();
  failedRequest.messages[1].content = "fail";
  try {
    const succeeded = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: request,
    });
    assert.equal(succeeded.status, 200);
    const failed = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: failedRequest,
    });
    assert.equal(failed.status, 502);
  } finally {
    await fixture.worker.close();
  }
  const log = await readFile(fixture.metricsPath, "utf8");
  assert.ok(!log.includes(secret));
  assert.ok(!log.includes(outputSecret));
  assert.ok(!log.includes(runnerSecret));
  assert.ok(!log.includes(TOKEN));
  const records = log.trim().split("\n").map((line) => JSON.parse(line));
  assert.equal(records.length, 2);
  assert.deepEqual(records.map((record) => record.outcome), ["succeeded", "failed"]);
  await rm(fixture.directory, { recursive: true, force: true });
});

test("metrics rotate as bounded JSONL files", async () => {
  const fixture = await createFixture({
    metricsMaxBytes: 1,
    metricsBackups: 2,
  });
  try {
    for (let index = 0; index < 2; index += 1) {
      const result = await responseJson(fixture.base, "/v1/frontier-completions", {
        method: "POST",
        body: completionRequest(),
      });
      assert.equal(result.status, 200);
    }
  } finally {
    await fixture.worker.close();
  }
  const current = (await readFile(fixture.metricsPath, "utf8")).trim();
  const rotated = (await readFile(`${fixture.metricsPath}.1`, "utf8")).trim();
  assert.equal(current.split("\n").length, 1);
  assert.equal(rotated.split("\n").length, 1);
  assert.equal(JSON.parse(current).outcome, "succeeded");
  assert.equal(JSON.parse(rotated).outcome, "succeeded");
  await rm(fixture.directory, { recursive: true, force: true });
});

test("runtime metrics write failure is observable without changing completion output", async () => {
  const fixture = await createFixture();
  try {
    const metricsDirectory = dirname(fixture.metricsPath);
    await rm(metricsDirectory, { recursive: true, force: true });
    await writeFile(metricsDirectory, "blocked", { mode: 0o600 });
    const result = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(result.status, 200);
    const metricsHealth = await waitForMetricsHealth(
      fixture,
      (body) => body.write_failures_total === 1 && body.pending_records === 0,
    );
    assert.deepEqual(metricsHealth, {
      schema_version: 1,
      instance_id: "test-worker-instance",
      worker_source_sha256: "a".repeat(64),
      status: "degraded",
      writable_verified: true,
      records_attempted: 1,
      records_written: 0,
      pending_records: 0,
      write_failures_total: 1,
      last_error_code: "metrics_write_failed",
    });
  } finally {
    await fixture.cleanup();
  }
});

test("startup fails closed when the metrics destination is unavailable", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-codex-worker-metrics-test-"));
  const blocker = join(directory, "blocker");
  await writeFile(blocker, "blocked", { mode: 0o600 });
  await assert.rejects(
    startWorker({
      token: TOKEN,
      port: 0,
      runner: fakeRunner(),
      metricsPath: join(blocker, "worker.jsonl"),
    }),
    (error) => error.code === "metrics_unavailable" && error.status === 503,
  );
  await rm(directory, { recursive: true, force: true });
});

test("startup requires verified ChatGPT login identity", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-codex-worker-login-test-"));
  await assert.rejects(
    startWorker({
      token: TOKEN,
      port: 0,
      metricsPath: join(directory, "metrics.jsonl"),
      runner: fakeRunner({
        verify: async () => verification({ auth_mode: "api_key" }),
      }),
    }),
    (error) => error.code === "chatgpt_login_required",
  );
  await assert.rejects(
    startWorker({
      token: TOKEN,
      port: 0,
      metricsPath: join(directory, "metrics-version.jsonl"),
      runner: fakeRunner({
        verify: async () => verification({ codex_cli_version: "codex-cli 0.145.0" }),
      }),
    }),
    (error) => error.code === "invalid_codex_version",
  );
  await rm(directory, { recursive: true, force: true });
});

test("Codex invocation is pinned, ephemeral, read-only, and tool-disabled", () => {
  const args = codexArguments("/tmp/work", "/tmp/schema", "/tmp/output");
  assert.deepEqual(args.slice(0, 9), [
    "exec",
    "--strict-config",
    "--ignore-user-config",
    "--ignore-rules",
    "--ephemeral",
    "--json",
    "--color",
    "never",
    "-C",
  ]);
  assert.ok(args.includes(MODEL));
  assert.ok(args.includes(`model_reasoning_effort=\"${REASONING_EFFORT}\"`));
  assert.ok(args.includes("approval_policy=\"never\""));
  assert.ok(args.includes("web_search=\"disabled\""));
  assert.ok(args.includes("read-only"));
  for (const feature of DISABLED_CODEX_FEATURES) {
    assert.ok(args.includes(feature));
  }
  assert.deepEqual(
    args.flatMap((value, index) => (args[index - 1] === "--disable" ? [value] : [])),
    DISABLED_CODEX_FEATURES,
  );
  assert.ok(!args.some((value) => value.includes("dangerously")));
  const request = completionRequest();
  const prompt = buildTrustedPrompt(request.messages, request.frontier);
  assert.ok(prompt.startsWith("You are the structured model frontier"));
  assert.ok(prompt.includes(`UNTRUSTED_MESSAGES_JSON:${JSON.stringify(request.messages)}`));
  assert.ok(prompt.includes(`TRUSTED_FRONTIER_NAME_JSON:${JSON.stringify(request.frontier.name)}`));
});

test("ChatGPT startup verification accepts the Codex status stream", () => {
  assert.equal(isChatGptLoginStatus("Logged in using ChatGPT", ""), true);
  assert.equal(isChatGptLoginStatus("", "Logged in using ChatGPT"), true);
  assert.equal(isChatGptLoginStatus("", "Logged in using API key"), false);
  assert.equal(isChatGptLoginStatus("prefix Logged in using ChatGPT", ""), false);
  assert.equal(
    isChatGptLoginStatus("Logged in using ChatGPT", "Logged in using ChatGPT"),
    false,
  );
  assert.equal(isSupportedCodexVersion(VERSION), true);
  assert.equal(isSupportedCodexVersion("codex-cli 0.145.0"), false);
});

test("Codex identity drift fails closed before and after execution", async () => {
  const request = completionRequest();
  let executions = 0;
  const before = createCodexRunner({
    captureProcess: captureIdentities([
      {},
      { login: "Logged in using API key" },
    ]),
    executeCodex: async () => {
      executions += 1;
      return "";
    },
  });
  await before.verify();
  await assert.rejects(
    before.complete({
      messages: request.messages,
      frontier: request.frontier,
      signal: new AbortController().signal,
    }),
    (error) => error.code === "codex_identity_changed",
  );
  assert.equal(executions, 0);

  const after = createCodexRunner({
    captureProcess: captureIdentities([
      {},
      {},
      { version: "codex-cli 0.145.0" },
    ]),
    executeCodex: async () => {
      executions += 1;
      return "";
    },
  });
  await after.verify();
  await assert.rejects(
    after.complete({
      messages: request.messages,
      frontier: request.frontier,
      signal: new AbortController().signal,
    }),
    (error) => error.code === "codex_identity_changed",
  );
  assert.equal(executions, 1);
});
