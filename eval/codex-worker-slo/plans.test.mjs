import assert from "node:assert/strict";
import test from "node:test";
import {
  SloPlanError,
  assertKnownPlan,
  assertPlanBudget,
  getPlan,
  planInvocationCount,
  planLiveCallCount,
  planNames,
} from "./plans.mjs";

test("closed plans have fixed call arithmetic", () => {
  assert.deepEqual(planNames(), [
    "development",
    "live_canary",
    "step_load_diagnostic",
    "commercial_candidate",
  ]);
  assert.equal(planLiveCallCount(getPlan("development")), 0);
  assert.equal(planInvocationCount(getPlan("development")), 12);
  assert.equal(planLiveCallCount(getPlan("live_canary")), 15);
  assert.equal(planInvocationCount(getPlan("live_canary")), 15);
  assert.equal(planLiveCallCount(getPlan("step_load_diagnostic")), 10);
  assert.equal(planLiveCallCount(getPlan("commercial_candidate")), 90);
  assert.doesNotThrow(() => assertPlanBudget(getPlan("live_canary")));
});

test("plan mutations and unknown plans fail closed", () => {
  const plan = getPlan("live_canary");
  plan.phases[1].waves = 5;
  assert.throws(
    () => assertKnownPlan(plan),
    (error) => error instanceof SloPlanError && error.code === "plan_not_registered",
  );
  assert.throws(
    () => getPlan("adhoc"),
    (error) => error instanceof SloPlanError && error.code === "unknown_plan",
  );
});
