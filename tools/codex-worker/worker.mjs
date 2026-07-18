import { createHash, randomUUID, timingSafeEqual } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  AdmissionRegistry,
  requestObservationId,
  validateObservationId,
} from "./admission-registry.mjs";
import { createCodexRunner, readKeychainToken } from "./codex-runner.mjs";
import { MetricsLog } from "./metrics-log.mjs";
import {
  AUTH_MODE,
  CODEX_CLI_VERSION,
  MODEL,
  PROVIDER,
  REASONING_EFFORT,
  WorkerError,
  completionEnvelope,
  healthEnvelope,
  validateCompletionRequest,
  validateRunnerResult,
} from "./protocol.mjs";
import { RequestTimeline } from "./request-timeline.mjs";
import { RequestCounters, Scheduler, abortReason } from "./scheduler.mjs";

const HOST = "127.0.0.1";
const DEFAULT_PORT = 18_181;
const DEFAULT_CONCURRENCY = 2;
const DEFAULT_QUEUE = 8;
const DEFAULT_TIMEOUT_MS = 55_000;
const DEFAULT_MAX_BODY_BYTES = 2_000_000;
const SOURCE_FILES = [
  "admission-registry.mjs",
  "codex-runner.mjs",
  "metrics-log.mjs",
  "protocol.mjs",
  "request-timeline.mjs",
  "scheduler.mjs",
  "worker.mjs",
];

export async function workerSourceSha256() {
  const digest = createHash("sha256");
  for (const name of SOURCE_FILES) {
    const content = await readFile(new URL(name, import.meta.url));
    digest.update(String(Buffer.byteLength(name)));
    digest.update(":" + name + ":" + String(content.length) + ":");
    digest.update(content);
  }
  return digest.digest("hex");
}

function json(response, status, value) {
  if (response.destroyed || response.writableEnded || response.writableFinished) {
    return false;
  }
  const body = JSON.stringify(value);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
    "cache-control": "no-store",
    "x-content-type-options": "nosniff",
  });
  response.end(body);
  return true;
}

function errorResponse(response, error) {
  const failure = errorDetails(error);
  json(response, failure.status, { error: { code: failure.code } });
  return failure;
}

function errorDetails(error) {
  const status = error instanceof WorkerError ? error.status : 502;
  const code = error instanceof WorkerError ? error.code : "runner_failure";
  return { status, code };
}

function bearerToken(request) {
  const authorization = request.headers.authorization;
  if (typeof authorization !== "string" || !authorization.startsWith("Bearer ")) {
    return null;
  }
  return authorization.slice(7);
}

function authorized(request, token) {
  const presented = bearerToken(request);
  if (presented === null) {
    return false;
  }
  const expectedBytes = Buffer.from(token);
  const presentedBytes = Buffer.from(presented);
  return expectedBytes.length === presentedBytes.length
    && timingSafeEqual(expectedBytes, presentedBytes);
}

function admissionQueryObservationId(request) {
  let url;
  try {
    url = new URL(request.url, "http://127.0.0.1");
  } catch {
    return null;
  }
  if (url.pathname !== "/request-admission") {
    return null;
  }
  const values = url.searchParams.getAll("observation_id");
  if (values.length !== 1
    || [...url.searchParams.keys()].some((key) => key !== "observation_id")) {
    throw new WorkerError("invalid_admission_query", 400);
  }
  return validateObservationId(values[0]);
}

async function readJsonBody(request, maxBytes) {
  const declared = Number(request.headers["content-length"]);
  if (Number.isFinite(declared) && declared > maxBytes) {
    throw new WorkerError("request_too_large", 413);
  }
  const chunks = [];
  let bytes = 0;
  let tooLarge = false;
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > maxBytes) {
      tooLarge = true;
      continue;
    }
    chunks.push(chunk);
  }
  if (tooLarge || bytes === 0) {
    throw new WorkerError(tooLarge ? "request_too_large" : "invalid_json", tooLarge ? 413 : 400);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch {
    throw new WorkerError("invalid_json", 400);
  }
}

function withTimeout(operation, timeoutMs, externalSignal) {
  const controller = new AbortController();
  const timeoutError = new WorkerError("codex_timeout", 504);
  return new Promise((resolvePromise, rejectPromise) => {
    let settled = false;
    let terminalError = null;
    let graceTimer = null;
    const finish = (error, value) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      if (graceTimer !== null) {
        clearTimeout(graceTimer);
      }
      externalSignal?.removeEventListener("abort", externalAbort);
      if (error) {
        rejectPromise(error);
      } else {
        resolvePromise(value);
      }
    };
    const abort = (error) => {
      if (terminalError !== null) {
        return;
      }
      terminalError = error;
      controller.abort(error);
      graceTimer = setTimeout(() => finish(error), 2_500);
    };
    const externalAbort = () => abort(abortReason(externalSignal));
    const timer = setTimeout(() => abort(timeoutError), timeoutMs);
    if (externalSignal?.aborted) {
      externalAbort();
    } else {
      externalSignal?.addEventListener("abort", externalAbort, { once: true });
    }
    Promise.resolve()
      .then(() => operation(controller.signal))
      .then(
        (value) => finish(terminalError, value),
        (error) => finish(terminalError ?? error),
      );
  });
}

function watchDisconnect(request, response) {
  const controller = new AbortController();
  const disconnect = () => {
    if (!response.writableEnded && !controller.signal.aborted) {
      controller.abort(new WorkerError("client_disconnected", 499));
    }
  };
  request.once("aborted", disconnect);
  response.once("close", disconnect);
  if (request.aborted || response.destroyed) {
    disconnect();
  }
  return {
    signal: controller.signal,
    cleanup() {
      request.removeListener("aborted", disconnect);
      response.removeListener("close", disconnect);
    },
  };
}

function parsePositiveInteger(value, fallback, minimum, maximum, name) {
  if (value === undefined || value === null || value === "") {
    return fallback;
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < minimum || parsed > maximum) {
    throw new WorkerError(`invalid_${name}`, 500);
  }
  return parsed;
}

async function resolveToken(options) {
  const configured = options.token ?? process.env.STARRING_CODEX_WORKER_TOKEN;
  const token = configured ?? await readKeychainToken({
    service: options.keychainService ?? process.env.STARRING_CODEX_WORKER_KEYCHAIN_SERVICE,
    account: options.keychainAccount ?? process.env.STARRING_CODEX_WORKER_KEYCHAIN_ACCOUNT,
    securityPath: options.securityPath,
    environment: options.environment,
  });
  if (typeof token !== "string" || token.length < 12 || token !== token.trim()) {
    throw new WorkerError("invalid_worker_token", 500);
  }
  return token;
}

function metricRecord(input) {
  return {
    metric_schema_version: 2,
    timestamp: new Date().toISOString(),
    request_id: input.requestId,
    instance_id: input.identity.instance_id,
    worker_source_sha256: input.identity.worker_source_sha256,
    provider: PROVIDER,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    concurrency_limit: input.concurrency,
    queue_capacity: input.maxQueue,
    request_timeout_ms: input.timeoutMs,
    frontier_name: input.frontierName,
    outcome: input.outcome,
    status_code: input.status,
    duration_ms: input.durationMs,
    ...input.timeline,
    usage: input.usage ?? null,
    error_code: input.errorCode ?? null,
  };
}

function scheduleCompletion(input) {
  const activeBefore = input.scheduler.active;
  const queuedBefore = input.scheduler.queue.length;
  input.timeline.admit(activeBefore, queuedBefore);
  let submitted;
  try {
    submitted = input.counters.submit(input.scheduler, async () => {
      input.admissions.activate(input.observationId);
      input.timeline.runnerStarted();
      try {
        const result = await input.runner.complete({
          requestId: input.requestId,
          model: MODEL,
          reasoningEffort: REASONING_EFFORT,
          messages: input.body.messages,
          frontier: input.body.frontier,
          signal: input.signal,
        });
        input.timeline.runnerSettled("resolved");
        return result;
      } catch (error) {
        input.timeline.runnerSettled("rejected");
        throw error;
      }
    }, input.signal);
  } catch (error) {
    input.timeline.submissionObserved(input.scheduler.active, input.scheduler.queue.length);
    throw error;
  }
  const activeAfter = input.scheduler.active;
  const queuedAfter = input.scheduler.queue.length;
  input.timeline.submissionObserved(activeAfter, queuedAfter);
  if (activeAfter > activeBefore) {
    input.admissions.admit(input.observationId, "active");
  } else if (queuedAfter > queuedBefore) {
    input.admissions.admit(input.observationId, "queued");
  }
  return Promise.resolve(submitted).finally(() => {
    input.admissions.release(input.observationId);
  });
}

async function listen(server, port) {
  await new Promise((resolvePromise, rejectPromise) => {
    const failed = (error) => rejectPromise(error);
    server.once("error", failed);
    server.listen(port, HOST, () => {
      server.removeListener("error", failed);
      resolvePromise();
    });
  });
}

export async function startWorker(options = {}) {
  if ((options.host ?? HOST) !== HOST) {
    throw new WorkerError("loopback_only", 500);
  }
  const port = parsePositiveInteger(
    options.port ?? process.env.STARRING_CODEX_WORKER_PORT,
    DEFAULT_PORT,
    0,
    65_535,
    "port",
  );
  const concurrency = parsePositiveInteger(
    options.concurrency ?? process.env.STARRING_CODEX_WORKER_CONCURRENCY,
    DEFAULT_CONCURRENCY,
    1,
    8,
    "concurrency",
  );
  const maxQueue = parsePositiveInteger(
    options.maxQueue ?? process.env.STARRING_CODEX_WORKER_MAX_QUEUE,
    DEFAULT_QUEUE,
    0,
    128,
    "queue",
  );
  const timeoutMs = parsePositiveInteger(
    options.timeoutMs ?? process.env.STARRING_CODEX_WORKER_TIMEOUT_MS,
    DEFAULT_TIMEOUT_MS,
    1,
    DEFAULT_TIMEOUT_MS,
    "timeout",
  );
  const maxBodyBytes = parsePositiveInteger(
    options.maxBodyBytes,
    DEFAULT_MAX_BODY_BYTES,
    1_024,
    8_000_000,
    "body_limit",
  );
  const token = await resolveToken(options);
  const runner = options.runner ?? createCodexRunner({
    codexPath: options.codexPath,
    environment: options.environment,
    tempRoot: options.tempRoot,
    maxOutputBytes: options.maxOutputBytes,
  });
  const verification = await runner.verify();
  if (verification?.auth_mode !== AUTH_MODE) {
    throw new WorkerError("chatgpt_login_required", 503);
  }
  if (verification.codex_cli_version !== CODEX_CLI_VERSION) {
    throw new WorkerError("invalid_codex_version", 503);
  }
  const instanceId = options.instanceId ?? randomUUID();
  if (typeof instanceId !== "string"
    || instanceId.length === 0
    || instanceId.length > 128
    || instanceId !== instanceId.trim()) {
    throw new WorkerError("invalid_instance_id", 500);
  }
  const sourceSha256 = options.workerSourceSha256 ?? await workerSourceSha256();
  if (typeof sourceSha256 !== "string" || !/^[0-9a-f]{64}$/.test(sourceSha256)) {
    throw new WorkerError("invalid_worker_source", 500);
  }
  const identity = Object.freeze({
    provider: PROVIDER,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    auth_mode: AUTH_MODE,
    codex_cli_version: verification.codex_cli_version,
    instance_id: instanceId,
    worker_source_sha256: sourceSha256,
  });
  const scheduler = new Scheduler(concurrency, maxQueue);
  const counters = new RequestCounters();
  const admissions = new AdmissionRegistry({ ttlMs: timeoutMs + 5_000 });
  const logPath = options.metricsPath
    ?? process.env.STARRING_CODEX_WORKER_METRICS_LOG
    ?? join(homedir(), "Library", "Logs", "Starring", "codex-worker.jsonl");
  const metrics = new MetricsLog(logPath, {
    maxBytes: options.metricsMaxBytes,
    backups: options.metricsBackups,
  });
  try {
    await metrics.verifyWritable();
  } catch {
    throw new WorkerError("metrics_unavailable", 503);
  }

  const handlers = new Set();
  const handleRequest = async (request, response) => {
    if (!authorized(request, token)) {
      json(response, 401, { error: { code: "unauthorized" } });
      return;
    }
    let admissionObservationId;
    try {
      admissionObservationId = admissionQueryObservationId(request);
    } catch (error) {
      errorResponse(response, error);
      return;
    }
    if (request.method === "GET" && admissionObservationId !== null) {
      const admission = admissions.lookup(admissionObservationId);
      if (admission === null) {
        errorResponse(response, new WorkerError("admission_not_found", 404));
      } else {
        json(response, 200, admission);
      }
      return;
    }
    if (request.method === "GET" && request.url === "/health") {
      const counterSnapshot = counters.snapshot(scheduler.active, scheduler.queue.length);
      json(response, 200, healthEnvelope(
        identity,
        scheduler.active,
        scheduler.queue.length,
        concurrency,
        maxQueue,
        timeoutMs,
        counterSnapshot.accepted,
        counterSnapshot.settled,
      ));
      return;
    }
    if (request.method === "GET" && request.url === "/metrics-health") {
      json(response, 200, {
        schema_version: 1,
        instance_id: identity.instance_id,
        worker_source_sha256: identity.worker_source_sha256,
        ...metrics.snapshot(),
      });
      return;
    }
    if (request.method !== "POST" || request.url !== "/v1/frontier-completions") {
      json(response, 404, { error: { code: "not_found" } });
      return;
    }
    let body;
    let observationId;
    try {
      body = validateCompletionRequest(await readJsonBody(request, maxBodyBytes));
      observationId = requestObservationId(request);
    } catch (error) {
      errorResponse(response, error);
      return;
    }
    const requestId = randomUUID();
    try {
      if (observationId !== null) {
        admissions.reserve(observationId, requestId);
      }
    } catch (error) {
      errorResponse(response, error);
      return;
    }
    const started = Date.now();
    const timeline = new RequestTimeline({ clock: options.timelineClock });
    const disconnect = watchDisconnect(request, response);
    try {
      const result = await withTimeout(
        (signal) => scheduleCompletion({
          admissions,
          body,
          counters,
          requestId,
          runner,
          scheduler,
          signal,
          timeline,
          observationId,
        }),
        timeoutMs,
        disconnect.signal,
      );
      timeline.resultValidationStarted();
      validateRunnerResult(result, identity);
      const durationMs = Date.now() - started;
      const timing = timeline.finish("completed");
      if (!disconnect.signal.aborted) {
        json(
          response,
          200,
          completionEnvelope(identity, requestId, body.frontier.name, result, durationMs),
        );
      }
      await metrics.record(metricRecord({
        concurrency,
        requestId,
        identity,
        frontierName: body.frontier.name,
        maxQueue,
        outcome: "succeeded",
        status: 200,
        durationMs,
        timeline: timing,
        timeoutMs,
        usage: result.usage,
      }));
    } catch (error) {
      const durationMs = Date.now() - started;
      const failure = errorDetails(error);
      const timing = timeline.finish(timeline.failureStage());
      if (!disconnect.signal.aborted) {
        json(response, failure.status, { error: { code: failure.code } });
      }
      await metrics.record(metricRecord({
        concurrency,
        requestId,
        identity,
        frontierName: body.frontier.name,
        maxQueue,
        outcome: "failed",
        status: failure.status,
        durationMs,
        timeline: timing,
        timeoutMs,
        errorCode: failure.code,
      }));
    } finally {
      admissions.release(observationId);
      disconnect.cleanup();
    }
  };
  const server = createServer((request, response) => {
    const handling = handleRequest(request, response);
    handlers.add(handling);
    handling.finally(() => handlers.delete(handling)).catch(() => {});
  });
  server.requestTimeout = DEFAULT_TIMEOUT_MS + 5_000;
  server.headersTimeout = 10_000;
  server.keepAliveTimeout = 5_000;
  await listen(server, port);
  const address = server.address();
  if (typeof address !== "object" || address === null || address.address !== HOST) {
    server.close();
    throw new WorkerError("loopback_bind_failed", 500);
  }
  let closePromise = null;
  const close = () => {
    if (closePromise !== null) {
      return closePromise;
    }
    scheduler.stop();
    closePromise = (async () => {
      await new Promise((resolvePromise) => server.close(() => resolvePromise()));
      await Promise.allSettled([...handlers]);
      await scheduler.idle();
      await metrics.flush();
    })();
    return closePromise;
  };
  return {
    address,
    close,
    identity,
    metrics,
    server,
    stats: () => ({ active: scheduler.active, queued: scheduler.queue.length }),
  };
}

async function runMain() {
  const worker = await startWorker();
  process.stdout.write(`${JSON.stringify({
    event: "ready",
    address: worker.address.address,
    port: worker.address.port,
    identity: worker.identity,
    identity_sha256: createHash("sha256")
      .update(JSON.stringify(worker.identity))
      .digest("hex"),
  })}\n`);
  let stopping = false;
  const stop = async () => {
    if (stopping) {
      return;
    }
    stopping = true;
    await worker.close();
  };
  process.once("SIGTERM", stop);
  process.once("SIGINT", stop);
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : null;
if (invokedPath === import.meta.url) {
  runMain().catch((error) => {
    const code = error instanceof WorkerError ? error.code : "startup_failed";
    process.stderr.write(`${JSON.stringify({ event: "startup_failed", code })}\n`);
    process.exitCode = 1;
  });
}
