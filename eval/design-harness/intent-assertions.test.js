const test = require('node:test');
const assert = require('node:assert/strict');

const checks = require('./intent-assertions');

function counters(overrides = {}) {
  return {
    route_calls: 0,
    proposal_acceptances: 0,
    resolution_acceptances: 0,
    compile_attempts: 0,
    compile_successes: 0,
    commits: 0,
    rollbacks: 0,
    conflicts: 0,
    stale_revision_rejections: 0,
    extraction_failures: 0,
    fallback_routes: {},
    ...overrides,
  };
}

function turn(overrides = {}) {
  return {
    id: 'build',
    input: 'Build it',
    outcome: 'ready',
    completed: true,
    message: 'Ready',
    question: null,
    halt_code: null,
    last_error: null,
    elapsed_ms: 1000,
    model_calls: 1,
    model_tool_calls: 1,
    deterministic_operations: 22,
    intent_counters: counters({
      route_calls: 1,
      proposal_acceptances: 1,
      compile_attempts: 1,
      compile_successes: 1,
      commits: 1,
    }),
    stage_before: 'empty',
    stage_after: 'preview_ready',
    intent_revision_before: 0,
    intent_revision_after: 1,
    draft_revision_before: 0,
    draft_revision_after: 22,
    draft_changed: true,
    actual_gates: { validation_current: true, simulation_current: true },
    restart_after: false,
    restart_performed: false,
    ...overrides,
  };
}

function receipt() {
  return {
    intent_revision: 1,
    candidate_revision: 22,
    input_intent_hash: '1'.repeat(64),
    semantic_intent_hash: '2'.repeat(64),
    compiled_plan_hash: '3'.repeat(64),
    compiled_operations: 22,
  };
}

function report(overrides = {}) {
  return JSON.stringify({
    schema_version: 3,
    input_schema_version: 3,
    mode: 'intent_recipe',
    requested_model: 'gemma4:12b-mlx',
    served_model: 'gemma4:12b-mlx',
    gateway_id: `sha256-${'9'.repeat(64)}`,
    declared_context_tokens: 16384,
    context_declaration_source: 'evaluation_provider',
    gateway_context_observed_tokens: null,
    provenance: {
      source_commit: 'a'.repeat(40),
      source_dirty: false,
      build_source_commit: 'a'.repeat(40),
      build_source_dirty: false,
      binary_sha256: 'b'.repeat(64),
      attestation_kind: 'local_unsigned',
      run_id: 'intent-test',
      run_order: 1,
      started_at_unix_ms: 100,
      ended_at_unix_ms: 1100,
    },
    oracle: { enabled: false, injected_control_calls: 0 },
    session_config: {
      max_model_calls: 12,
      max_tool_calls: 24,
      max_gate_failures: 4,
      context_char_budget: 44000,
    },
    outcome: 'ready',
    completed: true,
    message: 'Ready',
    question: null,
    halt_code: null,
    turns: [turn()],
    draft_revision: 22,
    ruleset: { version: 1, panels: [], modals: [], rules: [] },
    actual_gates: { validation_current: true, simulation_current: true },
    observability: { model_calls: 1, tool_calls: 1 },
    final_intent: {
      status: 'preview_ready',
      receipt: receipt(),
      public_status: { status: 'preview_ready', receipt: receipt() },
      binding_fingerprint: '4'.repeat(64),
    },
    persistence: {
      backend: 'sqlite_file',
      store_writes: 1,
      connection_reopen_count: 0,
      final_generation: 1,
      snapshot_schema_version: 6,
      roundtrip_verified: false,
    },
    elapsed_ms: 1000,
    ...overrides,
  });
}

function context(overrides = {}) {
  return {
    vars: {
      expectedOutcomes: 'ready',
      expectedStagePath: 'empty>preview_ready',
      expectedRoutePath: 'private_study_room',
      expectedFinalStatus: 'preview_ready',
      expectedCompiledOperations: 22,
      completeRequest: true,
      expectedRestartCount: 0,
      ...overrides,
    },
  };
}

test('one-shot intent report satisfies provenance, route, receipt, call, and isolation contracts', () => {
  const document = report();
  const expected = context();

  assert.equal(checks.intentProvenance(document, expected).pass, true);
  assert.equal(checks.intentRouteStage(document, expected).pass, true);
  assert.equal(checks.intentReceipt(document, expected).pass, true);
  assert.equal(checks.intentOneCallTurns(document, expected).pass, true);
  assert.equal(checks.intentOracleIsolation(document, expected).pass, true);
  assert.equal(checks.intentDecisionFlow(document, expected).pass, true);
  assert.equal(checks.intentRestartContinuity(document, expected).pass, true);
  assert.equal(checks.intentHardLatency(document, expected).pass, true);
});

test('decision and restart assertions require a mutation-free pending turn and durable continuity', () => {
  const pending = turn({
    id: 'request',
    outcome: 'needs_input',
    completed: false,
    question: 'Which existing hub channel?',
    deterministic_operations: 0,
    intent_counters: counters({ route_calls: 1, proposal_acceptances: 1 }),
    stage_after: 'awaiting_decision',
    intent_revision_after: 1,
    draft_revision_after: 0,
    draft_changed: false,
    actual_gates: { validation_current: false, simulation_current: false },
    restart_after: true,
    restart_performed: true,
  });
  const resolved = turn({
    id: 'hub',
    input: 'Use community_hub',
    intent_counters: counters({
      resolution_acceptances: 1,
      compile_attempts: 1,
      compile_successes: 1,
      commits: 1,
    }),
    stage_before: 'awaiting_decision',
    intent_revision_before: 1,
    intent_revision_after: 2,
  });
  const document = report({
    turns: [pending, resolved],
    observability: { model_calls: 2, tool_calls: 2 },
    persistence: {
      backend: 'sqlite_file',
      store_writes: 2,
      connection_reopen_count: 1,
      final_generation: 2,
      snapshot_schema_version: 6,
      roundtrip_verified: true,
    },
    elapsed_ms: 2000,
  });
  const expected = context({
    expectedOutcomes: 'needs_input,ready',
    expectedStagePath: 'empty>awaiting_decision,awaiting_decision>preview_ready',
    expectedRoutePath: 'private_study_room,resolve_intent_decision',
    completeRequest: false,
    requiresDecision: true,
    expectedRestartCount: 1,
  });

  assert.equal(checks.intentRouteStage(document, expected).pass, true);
  assert.equal(checks.intentDecisionFlow(document, expected).pass, true);
  assert.equal(checks.intentRestartContinuity(document, expected).pass, true);

  const broken = JSON.parse(document);
  broken.turns[0].draft_changed = true;
  assert.match(checks.intentDecisionFlow(JSON.stringify(broken), expected).reason, /mutated/);
  broken.turns[0].draft_changed = false;
  broken.turns[1].intent_revision_before = 0;
  assert.match(checks.intentRestartContinuity(JSON.stringify(broken), expected).reason, /did not survive/);
});

test('fallback assertions require one explicit route and zero Draft mutation', () => {
  const routed = turn({
    id: 'custom',
    outcome: 'routed',
    completed: false,
    deterministic_operations: 0,
    intent_counters: counters({ route_calls: 1, fallback_routes: { typed_planner: 1 } }),
    stage_after: 'empty',
    intent_revision_after: 0,
    draft_revision_after: 0,
    draft_changed: false,
    actual_gates: { validation_current: false, simulation_current: false },
  });
  const document = report({
    outcome: 'routed',
    completed: false,
    turns: [routed],
    draft_revision: 0,
    actual_gates: { validation_current: false, simulation_current: false },
    final_intent: {
      status: 'empty',
      receipt: null,
      public_status: { status: 'empty', expected_revision: 0 },
      binding_fingerprint: '4'.repeat(64),
    },
  });
  const expected = context({
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'typed_planner',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
    noMutationTurns: 'custom',
  });

  assert.equal(checks.intentRouteStage(document, expected).pass, true);
  assert.equal(checks.intentReceipt(document, expected).pass, true);
  assert.equal(checks.intentNoMutationFallback(document, expected).pass, true);

  const broken = JSON.parse(document);
  broken.turns[0].intent_counters.commits = 1;
  assert.match(checks.intentNoMutationFallback(JSON.stringify(broken), expected).reason, /mutated or compiled/);
});

test('schema, model, oracle, calls, and hard latency regressions fail closed', () => {
  const malformed = JSON.parse(report());
  malformed.schema_version = 2;
  assert.match(checks.intentReceipt(JSON.stringify(malformed), context()).reason, /must be 3/);

  const wrongModel = JSON.parse(report());
  wrongModel.served_model = 'other';
  assert.match(checks.intentProvenance(JSON.stringify(wrongModel), context()).reason, /served_model/);

  const oracle = JSON.parse(report());
  oracle.oracle.enabled = true;
  assert.match(checks.intentOracleIsolation(JSON.stringify(oracle), context()).reason, /oracle=/);

  const calls = JSON.parse(report());
  calls.turns[0].model_calls = 2;
  assert.match(checks.intentOneCallTurns(JSON.stringify(calls), context()).reason, /expected=1\/1/);

  const latency = JSON.parse(report());
  latency.turns[0].elapsed_ms = 60001;
  assert.match(checks.intentHardLatency(JSON.stringify(latency), context()).reason, /exceeded/);
});
