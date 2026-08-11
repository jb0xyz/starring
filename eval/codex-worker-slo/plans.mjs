import { createHash } from "node:crypto";

const IDENTITY = Object.freeze({
  provider: "codex_chatgpt",
  model: "gpt-5.6-luna",
  reasoning_effort: "medium",
  auth_mode: "chatgpt",
  codex_cli_version: "codex-cli 0.147.0-alpha.6.5",
});

const ROUTINE_PROFILE = Object.freeze({
  concurrency_limit: 2,
  queue_capacity: 8,
  request_timeout_ms: 55_000,
});

const PLANS = {
  development: {
    schema_version: 1,
    id: "development",
    revision: 1,
    claim_scope: "development_only",
    execution_mode: "fake_only",
    identity: IDENTITY,
    worker_profile: ROUTINE_PROFILE,
    budgets: {
      live_calls: 0,
      input_tokens: 0,
      output_tokens: 0,
      max_duration_ms: 60_000,
    },
    resource_sampling: {
      interval_ms: 25,
      minimum_duration_ms: 0,
      maximum_samples: 2_500,
    },
    phases: [
      {
        id: "fake_transport",
        workload_id: "transport_probe",
        concurrency: 4,
        waves: 3,
        warmup: false,
        expected_outcome: "completed",
        calls_per_invocation: 1,
      },
    ],
    required_scenarios: [
      "queue_overflow",
      "cancellation",
      "deadline_exhaustion",
      "ignored_abort",
      "shutdown",
      "metrics_failure",
      "authentication_drift",
      "restart",
    ],
  },
  live_canary: {
    schema_version: 1,
    id: "live_canary",
    revision: 1,
    claim_scope: "pre_slo_diagnostic",
    execution_mode: "live",
    identity: IDENTITY,
    worker_profile: ROUTINE_PROFILE,
    budgets: {
      live_calls: 15,
      input_tokens: 130_000,
      output_tokens: 5_000,
      max_duration_ms: 15 * 60_000,
    },
    resource_sampling: {
      interval_ms: 250,
      minimum_duration_ms: 0,
      maximum_samples: 4_000,
    },
    phases: [
      {
        id: "warmup",
        workload_id: "transport_probe",
        concurrency: 1,
        waves: 1,
        warmup: true,
        expected_outcome: "completed",
        calls_per_invocation: 1,
      },
      {
        id: "serial",
        workload_id: "transport_probe",
        concurrency: 1,
        waves: 4,
        warmup: false,
        expected_outcome: "completed",
        calls_per_invocation: 1,
      },
      {
        id: "parallel_two",
        workload_id: "transport_probe",
        concurrency: 2,
        waves: 4,
        warmup: false,
        expected_outcome: "completed",
        calls_per_invocation: 1,
      },
      {
        id: "cancellation",
        workload_id: "transport_cancellation",
        concurrency: 1,
        waves: 1,
        warmup: false,
        expected_outcome: "cancelled",
        calls_per_invocation: 1,
      },
      {
        id: "post_cancellation_recovery",
        workload_id: "transport_probe",
        concurrency: 1,
        waves: 1,
        warmup: false,
        expected_outcome: "completed",
        calls_per_invocation: 1,
      },
    ],
    required_scenarios: ["cancellation"],
  },
  step_load_diagnostic: {
    schema_version: 1,
    id: "step_load_diagnostic",
    revision: 1,
    claim_scope: "capacity_diagnostic",
    execution_mode: "live",
    identity: IDENTITY,
    worker_profile: ROUTINE_PROFILE,
    budgets: {
      live_calls: 10,
      input_tokens: 100_000,
      output_tokens: 4_000,
      max_duration_ms: 15 * 60_000,
    },
    resource_sampling: {
      interval_ms: 250,
      minimum_duration_ms: 0,
      maximum_samples: 4_000,
    },
    phases: [1, 2, 3, 4].map((concurrency) => ({
      id: `concurrency_${concurrency}`,
      workload_id: "transport_probe",
      concurrency,
      waves: 1,
      warmup: false,
      expected_outcome: "completed",
      calls_per_invocation: 1,
    })),
    required_scenarios: [],
  },
  commercial_candidate: {
    schema_version: 1,
    id: "commercial_candidate",
    revision: 1,
    claim_scope: "commercial_candidate",
    execution_mode: "live",
    identity: IDENTITY,
    worker_profile: ROUTINE_PROFILE,
    budgets: {
      live_calls: 90,
      input_tokens: 1_500_000,
      output_tokens: 100_000,
      max_duration_ms: 8 * 60 * 60_000,
    },
    resource_sampling: {
      interval_ms: 5_000,
      minimum_duration_ms: 6 * 60 * 60_000,
      maximum_samples: 6_000,
    },
    phases: [
      {
        id: "one_call_concurrency_one",
        workload_id: "starring_v15_one_call",
        concurrency: 1,
        waves: 30,
        warmup: false,
        expected_outcome: "completed",
        calls_per_invocation: 1,
      },
      {
        id: "two_call_concurrency_two",
        workload_id: "starring_v15_two_call",
        concurrency: 2,
        waves: 15,
        warmup: false,
        expected_outcome: "completed",
        calls_per_invocation: 2,
      },
    ],
    required_scenarios: [
      "queue_overflow",
      "cancellation",
      "deadline_exhaustion",
      "restart",
    ],
  },
};

function stable(value) {
  if (Array.isArray(value)) {
    return value.map(stable);
  }
  if (value === null || typeof value !== "object") {
    return value;
  }
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
}

function deepFreeze(value) {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
  }
  return value;
}

for (const plan of Object.values(PLANS)) {
  deepFreeze(plan);
}

export class SloPlanError extends Error {
  constructor(code) {
    super(code);
    this.name = "SloPlanError";
    this.code = code;
  }
}

export function canonicalJson(value) {
  return JSON.stringify(stable(value));
}

export function planDigest(plan) {
  return createHash("sha256").update(canonicalJson(plan)).digest("hex");
}

export function planNames() {
  return Object.keys(PLANS);
}

export function getPlan(name) {
  const plan = PLANS[name];
  if (!plan) {
    throw new SloPlanError("unknown_plan");
  }
  return structuredClone(plan);
}

export function assertKnownPlan(plan) {
  if (!plan || typeof plan !== "object" || Array.isArray(plan)) {
    throw new SloPlanError("invalid_plan");
  }
  const expected = PLANS[plan.id];
  if (!expected || planDigest(plan) !== planDigest(expected)) {
    throw new SloPlanError("plan_not_registered");
  }
  return plan;
}

export function planInvocationCount(plan) {
  assertKnownPlan(plan);
  return plan.phases.reduce(
    (total, phase) => total + (phase.concurrency * phase.waves),
    0,
  );
}

export function planLiveCallCount(plan) {
  assertKnownPlan(plan);
  if (plan.execution_mode !== "live") {
    return 0;
  }
  return plan.phases.reduce(
    (total, phase) => total + (phase.concurrency * phase.waves * phase.calls_per_invocation),
    0,
  );
}

export function assertPlanBudget(plan) {
  assertKnownPlan(plan);
  const calls = planLiveCallCount(plan);
  if (calls > plan.budgets.live_calls) {
    throw new SloPlanError("plan_live_call_budget_exceeded");
  }
  if (plan.id === "live_canary" && calls !== 15) {
    throw new SloPlanError("canary_call_count_mismatch");
  }
  return plan;
}

export const EXPECTED_IDENTITY = IDENTITY;
export const EXPECTED_ROUTINE_PROFILE = ROUTINE_PROFILE;
