import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  buildTrustedPrompt,
  codexArguments,
  isChatGptLoginStatus,
} from "./codex-runner.mjs";
import {
  AUTH_MODE,
  MODEL,
  PROVIDER,
  REASONING_EFFORT,
} from "./protocol.mjs";
import { startWorker } from "./worker.mjs";

const TOKEN = "test-worker-token-1234567890";
const VERSION = "codex-cli 0.144.2";

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

function deferred() {
  let resolvePromise;
  const promise = new Promise((resolveValue) => {
    resolvePromise = resolveValue;
  });
  return { promise, resolve: resolvePromise };
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

    const health = await responseJson(fixture.base, "/health");
    assert.equal(health.status, 200);
    assert.deepEqual(sortedKeys(health.body), [
      "active_requests",
      "auth_mode",
      "codex_cli_version",
      "model",
      "provider",
      "queued_requests",
      "reasoning_effort",
      "schema_version",
      "status",
    ]);
    assert.deepEqual(health.body, {
      schema_version: 1,
      status: "ok",
      provider: PROVIDER,
      model: MODEL,
      reasoning_effort: REASONING_EFFORT,
      auth_mode: AUTH_MODE,
      codex_cli_version: VERSION,
      active_requests: 0,
      queued_requests: 0,
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
  } finally {
    await fixture.cleanup();
  }
});

test("successful completion returns the exact native envelope", async () => {
  const calls = [];
  const fixture = await createFixture({
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
    const third = responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    await waitFor(() => fixture.worker.stats().queued === 1);
    const overflow = await responseJson(fixture.base, "/v1/frontier-completions", {
      method: "POST",
      body: completionRequest(),
    });
    assert.equal(overflow.status, 429);
    assert.deepEqual(overflow.body, { error: { code: "queue_full" } });
    assert.equal(calls, 2);
    gate.resolve();
    const accepted = await Promise.all([first, second, third]);
    assert.deepEqual(accepted.map((entry) => entry.status), [200, 200, 200]);
    assert.equal(calls, 3);
  } finally {
    gate.resolve();
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
  } finally {
    await failed.cleanup();
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
  await rm(directory, { recursive: true, force: true });
});

test("Codex invocation is pinned, ephemeral, read-only, and tool-disabled", () => {
  const args = codexArguments("/tmp/work", "/tmp/schema", "/tmp/output");
  assert.deepEqual(args.slice(0, 8), [
    "exec",
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
  for (const feature of [
    "shell_tool",
    "apps",
    "goals",
    "hooks",
    "multi_agent",
    "remote_plugin",
    "memories",
  ]) {
    assert.ok(args.includes(feature));
  }
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
});
