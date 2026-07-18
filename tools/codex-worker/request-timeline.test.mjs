import assert from "node:assert/strict";
import test from "node:test";
import { RequestTimeline } from "./request-timeline.mjs";

function clock(milliseconds) {
  let index = 0;
  return () => {
    const value = milliseconds[index];
    index += 1;
    return BigInt(value) * 1_000_000n;
  };
}

function admit(timeline, before, after) {
  timeline.admit(before.active, before.queued);
  timeline.submissionObserved(after.active, after.queued);
}

test("timeline decomposes queue, runner, and validation time exactly", () => {
  const timeline = new RequestTimeline({ clock: clock([100, 125, 190, 197]) });
  admit(timeline, { active: 2, queued: 1 }, { active: 2, queued: 2 });
  timeline.runnerStarted();
  assert.equal(timeline.runnerSettled("resolved"), true);
  timeline.resultValidationStarted();
  assert.deepEqual(timeline.finish("completed"), {
    active_at_admission: 2,
    queued_at_admission: 1,
    queue_wait_ms: 25,
    runner_duration_ms: 65,
    runner_elapsed_at_terminal_ms: null,
    post_runner_ms: 7,
    total_duration_ms: 97,
    runner_started: true,
    runner_settled: true,
    runner_outcome: "resolved",
    result_validation_started: true,
    terminal_stage: "completed",
  });
});

test("timeline records rejected admission without queue or runner time", () => {
  const timeline = new RequestTimeline({ clock: clock([10, 13]) });
  admit(timeline, { active: 2, queued: 1 }, { active: 2, queued: 1 });
  assert.equal(timeline.failureStage(), "admission");
  assert.deepEqual(timeline.finish("admission"), {
    active_at_admission: 2,
    queued_at_admission: 1,
    queue_wait_ms: 0,
    runner_duration_ms: null,
    runner_elapsed_at_terminal_ms: null,
    post_runner_ms: 3,
    total_duration_ms: 3,
    runner_started: false,
    runner_settled: false,
    runner_outcome: null,
    result_validation_started: false,
    terminal_stage: "admission",
  });
});

test("timeline distinguishes queued and active timeout failures", () => {
  const queued = new RequestTimeline({ clock: clock([30, 50]) });
  admit(queued, { active: 1, queued: 0 }, { active: 1, queued: 1 });
  assert.equal(queued.failureStage(), "queue");
  assert.equal(queued.finish("queue").queue_wait_ms, 20);

  const active = new RequestTimeline({ clock: clock([60, 65, 90, 91]) });
  admit(active, { active: 0, queued: 0 }, { active: 1, queued: 0 });
  active.runnerStarted();
  assert.equal(active.runnerSettled("rejected"), true);
  assert.equal(active.failureStage(), "runner");
  assert.deepEqual(active.finish("runner"), {
    active_at_admission: 0,
    queued_at_admission: 0,
    queue_wait_ms: 5,
    runner_duration_ms: 25,
    runner_elapsed_at_terminal_ms: null,
    post_runner_ms: 1,
    total_duration_ms: 31,
    runner_started: true,
    runner_settled: true,
    runner_outcome: "rejected",
    result_validation_started: false,
    terminal_stage: "runner",
  });
});

test("timeline classifies rejected validated runner output separately", () => {
  const timeline = new RequestTimeline({ clock: clock([1, 2, 4, 8]) });
  admit(timeline, { active: 0, queued: 0 }, { active: 1, queued: 0 });
  timeline.runnerStarted();
  timeline.runnerSettled("resolved");
  assert.equal(timeline.failureStage(), "runner");
  timeline.resultValidationStarted();
  assert.equal(timeline.failureStage(), "result_validation");
  assert.equal(timeline.finish("result_validation").post_runner_ms, 4);
});

test("late runner settlement cannot mutate a censored terminal record", () => {
  const timeline = new RequestTimeline({ clock: clock([5, 6, 20]) });
  admit(timeline, { active: 0, queued: 0 }, { active: 1, queued: 0 });
  timeline.runnerStarted();
  const result = timeline.finish("runner");
  assert.equal(result.runner_settled, false);
  assert.equal(result.runner_duration_ms, null);
  assert.equal(result.runner_elapsed_at_terminal_ms, 14);
  assert.equal(timeline.runnerSettled("resolved"), false);
  assert.equal(timeline.finish("runner"), result);
});

test("timeline keeps exact decomposition across sub-millisecond boundaries", () => {
  const values = [0n, 1_900_000n, 3_100_000n, 4_999_999n];
  const timeline = new RequestTimeline({ clock: () => values.shift() });
  admit(timeline, { active: 0, queued: 0 }, { active: 1, queued: 0 });
  timeline.runnerStarted();
  timeline.runnerSettled("resolved");
  timeline.resultValidationStarted();
  const result = timeline.finish("completed");
  assert.equal(result.queue_wait_ms, 1);
  assert.equal(result.runner_duration_ms, 2);
  assert.equal(result.post_runner_ms, 1);
  assert.equal(result.total_duration_ms, 4);
  assert.equal(
    result.queue_wait_ms + result.runner_duration_ms + result.post_runner_ms,
    result.total_duration_ms,
  );
});

test("timeline rejects invalid clocks and transitions", () => {
  const invalid = new RequestTimeline({ clock: () => Number.NaN });
  assert.throws(() => invalid.admit(0, 0), /invalid_monotonic_timestamp/);
  const backwards = new RequestTimeline({ clock: clock([10, 9]) });
  backwards.admit(0, 0);
  backwards.submissionObserved(1, 0);
  assert.throws(() => backwards.runnerStarted(), /non_monotonic_timestamp/);
  const missingStart = new RequestTimeline({ clock: clock([1]) });
  assert.throws(
    () => missingStart.runnerSettled("resolved"),
    /invalid_runner_settlement_transition/,
  );
});
