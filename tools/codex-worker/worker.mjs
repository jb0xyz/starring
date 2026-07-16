import { createHash, randomUUID, timingSafeEqual } from "node:crypto";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createCodexRunner, readKeychainToken } from "./codex-runner.mjs";
import { MetricsLog } from "./metrics-log.mjs";
import {
  AUTH_MODE,
  MODEL,
  PROVIDER,
  REASONING_EFFORT,
  WorkerError,
  completionEnvelope,
  healthEnvelope,
  validateCompletionRequest,
  validateRunnerResult,
} from "./protocol.mjs";

const HOST = "127.0.0.1";
const DEFAULT_PORT = 18_181;
const DEFAULT_CONCURRENCY = 2;
const DEFAULT_QUEUE = 8;
const DEFAULT_TIMEOUT_MS = 55_000;
const DEFAULT_MAX_BODY_BYTES = 2_000_000;

class Scheduler {
  constructor(concurrency, maxQueue) {
    this.concurrency = concurrency;
    this.maxQueue = maxQueue;
    this.active = 0;
    this.queue = [];
    this.accepting = true;
    this.idleWaiters = [];
  }

  submit(task) {
    if (!this.accepting) {
      return Promise.reject(new WorkerError("worker_shutting_down", 503));
    }
    if (this.active < this.concurrency) {
      return this.start(task);
    }
    if (this.queue.length >= this.maxQueue) {
      return Promise.reject(new WorkerError("queue_full", 429));
    }
    return new Promise((resolvePromise, rejectPromise) => {
      this.queue.push({ task, resolvePromise, rejectPromise });
    });
  }

  start(task) {
    this.active += 1;
    return Promise.resolve()
      .then(task)
      .finally(() => this.release());
  }

  release() {
    this.active -= 1;
    const next = this.queue.shift();
    if (next) {
      this.start(next.task).then(next.resolvePromise, next.rejectPromise);
      return;
    }
    if (this.active === 0) {
      const waiters = this.idleWaiters.splice(0);
      waiters.forEach((resolvePromise) => resolvePromise());
    }
  }

  stop() {
    this.accepting = false;
    const queued = this.queue.splice(0);
    queued.forEach(({ rejectPromise }) => {
      rejectPromise(new WorkerError("worker_shutting_down", 503));
    });
  }

  idle() {
    if (this.active === 0 && this.queue.length === 0) {
      return Promise.resolve();
    }
    return new Promise((resolvePromise) => this.idleWaiters.push(resolvePromise));
  }
}

function json(response, status, value) {
  const body = JSON.stringify(value);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
    "cache-control": "no-store",
    "x-content-type-options": "nosniff",
  });
  response.end(body);
}

function errorResponse(response, error) {
  const status = error instanceof WorkerError ? error.status : 502;
  const code = error instanceof WorkerError ? error.code : "runner_failure";
  json(response, status, { error: { code } });
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

function withTimeout(operation, timeoutMs) {
  const controller = new AbortController();
  const timeoutError = new WorkerError("codex_timeout", 504);
  return new Promise((resolvePromise, rejectPromise) => {
    let settled = false;
    let timedOut = false;
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
      if (error) {
        rejectPromise(error);
      } else {
        resolvePromise(value);
      }
    };
    const timer = setTimeout(() => {
      timedOut = true;
      controller.abort();
      graceTimer = setTimeout(() => finish(timeoutError), 2_500);
    }, timeoutMs);
    Promise.resolve()
      .then(() => operation(controller.signal))
      .then(
        (value) => finish(timedOut ? timeoutError : null, value),
        (error) => finish(timedOut ? timeoutError : error),
      );
  });
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
    timestamp: new Date().toISOString(),
    request_id: input.requestId,
    provider: PROVIDER,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    frontier_name: input.frontierName,
    outcome: input.outcome,
    status_code: input.status,
    duration_ms: input.durationMs,
    usage: input.usage ?? null,
    error_code: input.errorCode ?? null,
  };
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
  if (verification?.auth_mode !== AUTH_MODE
    || typeof verification.codex_cli_version !== "string"
    || verification.codex_cli_version.length === 0) {
    throw new WorkerError("chatgpt_login_required", 503);
  }
  const identity = Object.freeze({
    provider: PROVIDER,
    model: MODEL,
    reasoning_effort: REASONING_EFFORT,
    auth_mode: AUTH_MODE,
    codex_cli_version: verification.codex_cli_version,
  });
  const scheduler = new Scheduler(concurrency, maxQueue);
  const logPath = options.metricsPath
    ?? process.env.STARRING_CODEX_WORKER_METRICS_LOG
    ?? join(homedir(), "Library", "Logs", "Starring", "codex-worker.jsonl");
  const metrics = new MetricsLog(logPath, {
    maxBytes: options.metricsMaxBytes,
    backups: options.metricsBackups,
  });

  const server = createServer(async (request, response) => {
    if (!authorized(request, token)) {
      json(response, 401, { error: { code: "unauthorized" } });
      return;
    }
    if (request.method === "GET" && request.url === "/health") {
      json(response, 200, healthEnvelope(identity, scheduler.active, scheduler.queue.length));
      return;
    }
    if (request.method !== "POST" || request.url !== "/v1/frontier-completions") {
      json(response, 404, { error: { code: "not_found" } });
      return;
    }
    let body;
    try {
      body = validateCompletionRequest(await readJsonBody(request, maxBodyBytes));
    } catch (error) {
      errorResponse(response, error);
      return;
    }
    const requestId = randomUUID();
    const started = Date.now();
    try {
      const result = await scheduler.submit(() => withTimeout(
        (signal) => runner.complete({
          requestId,
          model: MODEL,
          reasoningEffort: REASONING_EFFORT,
          messages: body.messages,
          frontier: body.frontier,
          signal,
        }),
        timeoutMs,
      ));
      validateRunnerResult(result, identity);
      const durationMs = Date.now() - started;
      json(
        response,
        200,
        completionEnvelope(identity, requestId, body.frontier.name, result, durationMs),
      );
      await metrics.record(metricRecord({
        requestId,
        frontierName: body.frontier.name,
        outcome: "succeeded",
        status: 200,
        durationMs,
        usage: result.usage,
      }));
    } catch (error) {
      const durationMs = Date.now() - started;
      const failure = errorResponse(response, error);
      await metrics.record(metricRecord({
        requestId,
        frontierName: body.frontier.name,
        outcome: "failed",
        status: failure.status,
        durationMs,
        errorCode: failure.code,
      }));
    }
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
    closePromise = Promise.all([
      new Promise((resolvePromise) => server.close(() => resolvePromise())),
      scheduler.idle(),
    ]).then(() => metrics.flush());
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
