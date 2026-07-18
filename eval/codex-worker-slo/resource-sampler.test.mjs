import assert from "node:assert/strict";
import test from "node:test";
import {
  ResourceSamplerError,
  collectProcessResourceSample,
  createResourceSampler,
  withResourceSampling,
} from "./resource-sampler.mjs";

test("process samples retain bounded numeric observations only", async () => {
  let invocation;
  const sample = await collectProcessResourceSample(12_345, {
    execFile: async (...args) => {
      invocation = args;
      return { stdout: "1024 12.5 01:02\n", stderr: "" };
    },
  });
  assert.equal(sample.pid, 12_345);
  assert.equal(sample.process_present, true);
  assert.equal(sample.rss_bytes, 1_048_576);
  assert.equal(sample.cpu_percent, 12.5);
  assert.equal(sample.process_elapsed_ms, 62_000);
  assert.equal(Object.hasOwn(sample, "command"), false);
  assert.equal(invocation[2].timeout, 2_000);
});

test("process sampling enforces a bounded ps timeout", async () => {
  let options;
  await collectProcessResourceSample(12_345, {
    psTimeoutMs: 25,
    execFile: async (_file, _arguments, received) => {
      options = received;
      return { stdout: "1024 0 00:01\n", stderr: "" };
    },
  });
  assert.equal(options.timeout, 25);
  await assert.rejects(
    collectProcessResourceSample(12_345, { psTimeoutMs: 0 }),
    (error) => error instanceof ResourceSamplerError && error.code === "invalid_ps_timeout",
  );
  await assert.rejects(
    collectProcessResourceSample(12_345, { psTimeoutMs: 30_001 }),
    (error) => error instanceof ResourceSamplerError && error.code === "invalid_ps_timeout",
  );
});

test("sampler is bounded and returns collection errors as codes", async () => {
  let clock = 0;
  let attempts = 0;
  const sampler = createResourceSampler({
    pid: process.pid,
    intervalMs: 1,
    maximumSamples: 2,
    clock: { now: () => clock },
    collect: async () => {
      attempts += 1;
      clock += 1;
      if (attempts === 1) {
        const error = new Error("hidden detail");
        error.code = "sample_unavailable";
        throw error;
      }
      return { pid: process.pid, process_present: true, rss_bytes: 10 };
    },
  });
  await sampler.start();
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 5));
  const result = await sampler.stop();
  assert.equal(result.errors[0].code, "sample_unavailable");
  assert.ok(result.samples.length <= 2);
  assert.equal(
    Object.hasOwn(result.samples[0], "evaluator_event_loop_delay_p99_ms"),
    true,
  );
  assert.equal(JSON.stringify(result).includes("hidden detail"), false);
  await assert.rejects(
    sampler.start(),
    (error) => error instanceof ResourceSamplerError && error.code === "sampler_already_started",
  );
  assert.throws(
    () => createResourceSampler({ maximumSamples: 10_001 }),
    (error) => error instanceof ResourceSamplerError && error.code === "invalid_sampler_limits",
  );
});

test("sampler replaces unsafe error codes without retaining error details", async () => {
  const sampler = createResourceSampler({
    intervalMs: 10,
    maximumSamples: 1,
    collect: async () => {
      const error = new Error("credential=hidden");
      error.code = "credential:do-not-store";
      throw error;
    },
  });
  await sampler.start();
  const result = await sampler.stop();
  assert.deepEqual(result.errors, [{
    at_ms: result.errors[0].at_ms,
    code: "resource_sample_failed",
  }]);
  assert.equal(JSON.stringify(result).includes("credential"), false);
  assert.equal(result.errors.length, 1);
});

test("withResourceSampling returns operation value and observations", async () => {
  const result = await withResourceSampling(async () => "done", {
    intervalMs: 10,
    maximumSamples: 1,
    collect: async () => ({ pid: process.pid, process_present: true, rss_bytes: 1 }),
  });
  assert.equal(result.value, "done");
  assert.equal(result.resources.samples.length, 1);
});

test("sampler propagates abort to a pending collector and stops scheduling", async () => {
  const controller = new AbortController();
  let collections = 0;
  const sampler = createResourceSampler({
    intervalMs: 1,
    maximumSamples: 10,
    collect: async (signal) => {
      collections += 1;
      await new Promise((resolvePromise) => {
        if (signal.aborted) {
          resolvePromise();
        } else {
          signal.addEventListener("abort", resolvePromise, { once: true });
        }
      });
      const error = new Error("aborted");
      error.code = "ABORT_ERR";
      throw error;
    },
  });
  const starting = sampler.start(controller.signal);
  controller.abort();
  await assert.rejects(
    starting,
    (error) => error instanceof ResourceSamplerError && error.code === "sampler_aborted",
  );
  const result = await sampler.stop();
  assert.equal(collections, 1);
  assert.equal(result.samples.length, 0);
  assert.equal(result.errors.length, 1);
});
