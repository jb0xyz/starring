import assert from "node:assert/strict";
import test from "node:test";
import { WorkerError } from "./protocol.mjs";
import { RequestCounters, Scheduler, abortReason } from "./scheduler.mjs";

function deferred() {
  let resolvePromise;
  const promise = new Promise((resolveValue) => {
    resolvePromise = resolveValue;
  });
  return { promise, resolve: resolvePromise };
}

test("scheduler starts queued tasks in FIFO order", async () => {
  const scheduler = new Scheduler(1, 2);
  const gate = deferred();
  const order = [];
  const first = scheduler.submit(async () => {
    order.push("first:start");
    await gate.promise;
    order.push("first:end");
    return "first";
  });
  const second = scheduler.submit(async () => {
    order.push("second");
    return "second";
  });
  const third = scheduler.submit(async () => {
    order.push("third");
    return "third";
  });

  assert.equal(scheduler.active, 1);
  assert.equal(scheduler.queue.length, 2);
  gate.resolve();

  assert.deepEqual(await Promise.all([first, second, third]), ["first", "second", "third"]);
  assert.deepEqual(order, ["first:start", "first:end", "second", "third"]);
  assert.equal(scheduler.active, 0);
  assert.equal(scheduler.queue.length, 0);
  await scheduler.idle();
});

test("scheduler enforces queue capacity with the transport error contract", async () => {
  const scheduler = new Scheduler(1, 1);
  const gate = deferred();
  let calls = 0;
  const first = scheduler.submit(async () => {
    calls += 1;
    await gate.promise;
    return "first";
  });
  const second = scheduler.submit(async () => {
    calls += 1;
    return "second";
  });

  await assert.rejects(
    scheduler.submit(async () => {
      calls += 1;
    }),
    (error) => error instanceof WorkerError
      && error.code === "queue_full"
      && error.status === 429,
  );
  assert.equal(scheduler.active, 1);
  assert.equal(scheduler.queue.length, 1);
  assert.equal(calls, 1);

  gate.resolve();
  assert.deepEqual(await Promise.all([first, second]), ["first", "second"]);
  assert.equal(calls, 2);
  await scheduler.idle();
});

test("aborting a queued entry removes it without starting its task", async () => {
  const scheduler = new Scheduler(1, 1);
  const gate = deferred();
  const controller = new AbortController();
  const timeout = new WorkerError("codex_timeout", 504);
  let queuedCalls = 0;
  const first = scheduler.submit(async () => {
    await gate.promise;
    return "first";
  });
  const queued = scheduler.submit(async () => {
    queuedCalls += 1;
  }, controller.signal);
  const rejected = assert.rejects(queued, (error) => error === timeout);

  assert.equal(scheduler.queue.length, 1);
  controller.abort(timeout);
  await rejected;

  assert.equal(scheduler.active, 1);
  assert.equal(scheduler.queue.length, 0);
  assert.equal(queuedCalls, 0);
  assert.equal(abortReason(controller.signal), timeout);

  gate.resolve();
  assert.equal(await first, "first");
  await scheduler.idle();
});

test("request counters preserve accepted and settled invariants", async () => {
  const scheduler = new Scheduler(1, 1);
  const counters = new RequestCounters();
  const gate = deferred();
  const first = counters.submit(scheduler, async () => {
    await gate.promise;
    return "first";
  });
  const second = counters.submit(scheduler, async () => "second");

  await assert.rejects(
    counters.submit(scheduler, async () => "overflow"),
    (error) => error instanceof WorkerError
      && error.code === "queue_full"
      && error.status === 429,
  );
  assert.deepEqual(counters.snapshot(scheduler.active, scheduler.queue.length), {
    accepted: 3,
    settled: 1,
  });
  assert.throws(
    () => counters.snapshot(0, 0),
    (error) => error instanceof WorkerError
      && error.code === "invalid_request_counters"
      && error.status === 500,
  );

  gate.resolve();
  assert.deepEqual(await Promise.all([first, second]), ["first", "second"]);
  assert.deepEqual(counters.snapshot(scheduler.active, scheduler.queue.length), {
    accepted: 3,
    settled: 3,
  });
});
