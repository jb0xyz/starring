import { execFile as execFileCallback } from "node:child_process";
import { cpus, freemem, loadavg, totalmem } from "node:os";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { promisify } from "node:util";

const execFile = promisify(execFileCallback);
const DEFAULT_PS_TIMEOUT_MS = 2_000;
const MAXIMUM_PS_TIMEOUT_MS = 30_000;
const MAXIMUM_SAMPLE_RECORDS = 10_000;

export class ResourceSamplerError extends Error {
  constructor(code) {
    super(code);
    this.name = "ResourceSamplerError";
    this.code = code;
  }
}

function boundedNumber(value) {
  return Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER
    ? value
    : null;
}

function boundedMilliseconds(value) {
  const bounded = boundedNumber(value);
  return bounded === null ? 0 : Math.round(bounded);
}

function safeErrorCode(error) {
  return typeof error?.code === "string" && /^[a-z][a-z0-9_]{0,127}$/.test(error.code)
    ? error.code
    : "resource_sample_failed";
}

function parseElapsed(value) {
  const pieces = value.trim().split(/[-:]/).map(Number);
  if (pieces.some((piece) => !Number.isFinite(piece))) {
    return null;
  }
  if (pieces.length === 4) {
    return (((pieces[0] * 24 + pieces[1]) * 60 + pieces[2]) * 60 + pieces[3]) * 1_000;
  }
  if (pieces.length === 3) {
    return ((pieces[0] * 60 + pieces[1]) * 60 + pieces[2]) * 1_000;
  }
  if (pieces.length === 2) {
    return (pieces[0] * 60 + pieces[1]) * 1_000;
  }
  return pieces.length === 1 ? pieces[0] * 1_000 : null;
}

export async function collectProcessResourceSample(pid, options = {}) {
  if (!Number.isSafeInteger(pid) || pid <= 0) {
    throw new ResourceSamplerError("invalid_pid");
  }
  const invoke = options.execFile ?? execFile;
  const psTimeoutMs = options.psTimeoutMs ?? DEFAULT_PS_TIMEOUT_MS;
  if (!Number.isSafeInteger(psTimeoutMs)
    || psTimeoutMs < 1
    || psTimeoutMs > MAXIMUM_PS_TIMEOUT_MS) {
    throw new ResourceSamplerError("invalid_ps_timeout");
  }
  let processValues = null;
  try {
    const result = await invoke("ps", ["-o", "rss=,pcpu=,etime=", "-p", String(pid)], {
      encoding: "utf8",
      maxBuffer: 8_192,
      signal: options.signal,
      timeout: psTimeoutMs,
    });
    const fields = result.stdout.trim().split(/\s+/);
    if (fields.length === 3) {
      processValues = {
        rss_bytes: boundedNumber(Number(fields[0]) * 1_024),
        cpu_percent: boundedNumber(Number(fields[1])),
        elapsed_ms: parseElapsed(fields[2]),
      };
    }
  } catch (error) {
    if (error?.code !== "ESRCH" && error?.code !== 1) {
      throw new ResourceSamplerError("process_sample_failed");
    }
  }
  const ownProcess = pid === process.pid;
  const memory = ownProcess ? process.memoryUsage() : null;
  const usage = ownProcess ? process.resourceUsage() : null;
  return {
    pid,
    process_present: processValues !== null,
    rss_bytes: processValues?.rss_bytes ?? (memory?.rss ?? null),
    heap_used_bytes: memory?.heapUsed ?? null,
    heap_total_bytes: memory?.heapTotal ?? null,
    external_bytes: memory?.external ?? null,
    cpu_percent: processValues?.cpu_percent ?? null,
    cpu_user_us: usage?.userCPUTime ?? null,
    cpu_system_us: usage?.systemCPUTime ?? null,
    process_elapsed_ms: processValues?.elapsed_ms ?? null,
    host_load_1m: boundedNumber(loadavg()[0]),
    host_free_memory_bytes: freemem(),
    host_total_memory_bytes: totalmem(),
    host_cpu_count: cpus().length,
  };
}

export function createResourceSampler(options = {}) {
  const pid = options.pid ?? process.pid;
  const intervalMs = options.intervalMs ?? 1_000;
  const maximumSamples = options.maximumSamples ?? 10_000;
  if (!Number.isSafeInteger(intervalMs) || intervalMs < 1
    || !Number.isSafeInteger(maximumSamples)
    || maximumSamples < 1
    || maximumSamples > MAXIMUM_SAMPLE_RECORDS) {
    throw new ResourceSamplerError("invalid_sampler_limits");
  }
  const clock = options.clock ?? { now: () => performance.now() };
  const collect = options.collect ?? ((signal) => collectProcessResourceSample(pid, { signal }));
  const eventLoop = monitorEventLoopDelay({ resolution: Math.min(intervalMs, 20) });
  const samples = [];
  const errors = [];
  let startedAt = null;
  let timer = null;
  let pending = Promise.resolve();
  let running = false;
  let runSignal = null;
  let abort = null;

  const take = async () => {
    if (samples.length + errors.length >= maximumSamples) {
      return;
    }
    const atMs = boundedMilliseconds(clock.now() - startedAt);
    try {
      const value = await collect(runSignal);
      const eventLoopDelay = eventLoop.percentile(99);
      samples.push({
        at_ms: atMs,
        ...value,
        evaluator_event_loop_delay_p99_ms: Number.isFinite(eventLoopDelay)
          ? eventLoopDelay / 1_000_000
          : null,
      });
      eventLoop.reset();
    } catch (error) {
      errors.push({
        at_ms: atMs,
        code: safeErrorCode(error),
      });
    }
  };

  const schedule = () => {
    if (!running || samples.length + errors.length >= maximumSamples) {
      return;
    }
    timer = setTimeout(() => {
      pending = take().finally(schedule);
    }, intervalMs);
  };

  return {
    async start(signal = null) {
      if (running || startedAt !== null) {
        throw new ResourceSamplerError("sampler_already_started");
      }
      if (signal !== null
        && (typeof signal !== "object"
          || typeof signal.addEventListener !== "function"
          || typeof signal.removeEventListener !== "function"
          || typeof signal.aborted !== "boolean")) {
        throw new ResourceSamplerError("invalid_sampler_signal");
      }
      running = true;
      startedAt = clock.now();
      runSignal = signal;
      abort = () => {
        running = false;
        if (timer !== null) {
          clearTimeout(timer);
          timer = null;
        }
      };
      runSignal?.addEventListener("abort", abort, { once: true });
      if (runSignal?.aborted) {
        abort();
      }
      eventLoop.enable();
      pending = take();
      await pending;
      if (runSignal?.aborted) {
        throw new ResourceSamplerError("sampler_aborted");
      }
      schedule();
    },
    async stop() {
      if (startedAt === null) {
        throw new ResourceSamplerError("sampler_not_started");
      }
      running = false;
      if (timer !== null) {
        clearTimeout(timer);
      }
      await pending;
      runSignal?.removeEventListener("abort", abort);
      abort = null;
      runSignal = null;
      eventLoop.disable();
      return {
        started_at_monotonic_ms: startedAt,
        duration_ms: boundedMilliseconds(clock.now() - startedAt),
        samples: structuredClone(samples),
        errors: structuredClone(errors),
      };
    },
  };
}

export async function withResourceSampling(operation, options = {}) {
  const sampler = createResourceSampler(options);
  await sampler.start();
  try {
    const value = await operation();
    return { value, resources: await sampler.stop() };
  } catch (error) {
    const resources = await sampler.stop();
    error.resource_samples = resources;
    throw error;
  }
}
