const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { createRequire } = require('node:module');
const path = require('node:path');
const vm = require('node:vm');
const { pathToFileURL } = require('node:url');

const checks = require('./intent-assertions');

const MANIFEST_DIGEST = '68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53';

function evidence(semanticPath = 'intent.automation_kind', description = 'Requested automation') {
  return { semantic_path: semanticPath, description };
}

function blocker(id, status, policyId = null) {
  return {
    id,
    status,
    policy_id: policyId,
    evidence: [evidence(`intent.runtime_requirements.${id}`, `Requires ${id}`)],
  };
}

function violation(id) {
  return {
    id,
    evidence: [evidence(`intent.boundary_requests.${id}`, `Requests ${id}`)],
  };
}

function decision(kind = 'private_study_room', overrides = {}) {
  return {
    kind,
    decision_source: 'deterministic_intent_adjudicator',
    adjudicator_version: 2,
    semantic_ir_digest: 'a'.repeat(64),
    manifest_version: 1,
    manifest_digest: MANIFEST_DIGEST,
    adjudication_digest: 'b'.repeat(64),
    blockers: [],
    boundary_violations: [],
    unclassified_requirements: [],
    route_target: kind === 'private_study_room'
      ? { recipe_id: 'starring.private_study_room', recipe_version: 1 }
      : null,
    ...overrides,
  };
}

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
    route_decision: decision(),
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
  const turns = overrides.turns || [turn()];
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
    turns,
    draft_revision: 22,
    ruleset: { version: 1, panels: [], modals: [], rules: [] },
    actual_gates: { validation_current: true, simulation_current: true },
    observability: { model_calls: 1, tool_calls: 1 },
    final_intent: {
      status: 'preview_ready',
      receipt: receipt(),
      public_status: { status: 'preview_ready', receipt: receipt() },
      route_decision: turns[turns.length - 1].route_decision,
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
      expectedBlockers: '',
      expectedBoundaryViolations: '',
      expectedUnclassifiedRequirements: '',
      ...overrides,
    },
  };
}

function routedDocument(routeDecision, message, id = 'routed') {
  const routed = turn({
    id,
    input: 'Route this request',
    outcome: 'routed',
    completed: false,
    message,
    deterministic_operations: 0,
    intent_counters: counters({
      route_calls: 1,
      fallback_routes: { [routeDecision.kind]: 1 },
    }),
    stage_after: 'empty',
    intent_revision_after: 0,
    route_decision: routeDecision,
    draft_revision_after: 0,
    draft_changed: false,
    actual_gates: { validation_current: false, simulation_current: false },
  });
  return report({
    outcome: 'routed',
    completed: false,
    message,
    turns: [routed],
    draft_revision: 0,
    actual_gates: { validation_current: false, simulation_current: false },
    final_intent: {
      status: 'empty',
      receipt: null,
      public_status: { status: 'empty', expected_revision: 0 },
      route_decision: routeDecision,
      binding_fingerprint: '4'.repeat(64),
    },
  });
}

function promptfooRealmChecks() {
  const filename = path.resolve(__dirname, 'intent-assertions.js');
  const module = { exports: {} };
  const sandbox = {
    Array,
    JSON,
    Object,
    console,
    module,
    exports: module.exports,
    require: createRequire(pathToFileURL(filename).href),
    __dirname: path.dirname(filename),
    __filename: filename,
  };
  vm.createContext(sandbox);
  const source = fs.readFileSync(filename, 'utf8');
  vm.runInContext(`(function (exports, require, module, __filename, __dirname) {${source}\n})(exports, require, module, __filename, __dirname);`, sandbox);
  return module.exports;
}

test('one-shot intent report satisfies provenance, route, receipt, call, and isolation contracts', () => {
  const document = report();
  const expected = context();

  assert.equal(checks.intentProvenance(document, expected).pass, true);
  assert.equal(checks.intentRouteStage(document, expected).pass, true);
  assert.equal(checks.intentReceipt(document, expected).pass, true);
  assert.equal(checks.intentOneCallTurns(document, expected).pass, true);
  assert.equal(checks.intentOracleIsolation(document, expected).pass, true);
  assert.equal(checks.intentAdjudicationDecision(document, expected).pass, true);
  assert.equal(checks.intentDecisionFlow(document, expected).pass, true);
  assert.equal(checks.intentRestartContinuity(document, expected).pass, true);
  assert.equal(checks.intentHardLatency(document, expected).pass, true);
});

test('structural assertions ignore key order and reject contract drift', () => {
  const reordered = JSON.parse(report());
  reordered.session_config = {
    context_char_budget: 44000,
    max_gate_failures: 4,
    max_model_calls: 12,
    max_tool_calls: 24,
  };
  const value = reordered.final_intent.public_status.receipt;
  reordered.final_intent.public_status.receipt = {
    compiled_operations: value.compiled_operations,
    compiled_plan_hash: value.compiled_plan_hash,
    semantic_intent_hash: value.semantic_intent_hash,
    input_intent_hash: value.input_intent_hash,
    candidate_revision: value.candidate_revision,
    intent_revision: value.intent_revision,
  };

  assert.equal(checks.intentProvenance(JSON.stringify(reordered), context()).pass, true);
  assert.equal(checks.intentReceipt(JSON.stringify(reordered), context()).pass, true);

  reordered.session_config.extra = true;
  assert.equal(checks.intentProvenance(JSON.stringify(reordered), context()).pass, false);
  delete reordered.session_config.extra;
  reordered.final_intent.public_status.receipt.compiled_operations = 23;
  assert.equal(checks.intentReceipt(JSON.stringify(reordered), context()).pass, false);
});

test('structural assertions remain exact in the Promptfoo VM realm', () => {
  const vmChecks = promptfooRealmChecks();
  const reordered = JSON.parse(report());
  reordered.session_config = {
    context_char_budget: 44000,
    max_gate_failures: 4,
    max_model_calls: 12,
    max_tool_calls: 24,
  };
  const value = reordered.final_intent.public_status.receipt;
  reordered.final_intent.public_status.receipt = {
    compiled_operations: value.compiled_operations,
    compiled_plan_hash: value.compiled_plan_hash,
    semantic_intent_hash: value.semantic_intent_hash,
    input_intent_hash: value.input_intent_hash,
    candidate_revision: value.candidate_revision,
    intent_revision: value.intent_revision,
  };

  assert.equal(vmChecks.intentProvenance(JSON.stringify(reordered), context()).pass, true);
  assert.equal(vmChecks.intentReceipt(JSON.stringify(reordered), context()).pass, true);

  reordered.session_config.extra = true;
  assert.equal(vmChecks.intentProvenance(JSON.stringify(reordered), context()).pass, false);
  delete reordered.session_config.extra;
  reordered.final_intent.public_status.receipt.compiled_operations = 23;
  assert.equal(vmChecks.intentReceipt(JSON.stringify(reordered), context()).pass, false);
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
  assert.equal(checks.intentAdjudicationDecision(document, expected).pass, true);

  const broken = JSON.parse(document);
  broken.turns[0].draft_changed = true;
  assert.match(checks.intentDecisionFlow(JSON.stringify(broken), expected).reason, /mutated/);
  broken.turns[0].draft_changed = false;
  broken.turns[1].intent_revision_before = 0;
  assert.match(checks.intentRestartContinuity(JSON.stringify(broken), expected).reason, /did not survive/);
  broken.turns[1].intent_revision_before = 1;
  broken.turns[1].route_decision.adjudication_digest = 'c'.repeat(64);
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(broken), expected).reason,
    /changed during resolution/,
  );
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
    route_decision: decision('typed_planner'),
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
      route_decision: routed.route_decision,
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

test('adjudication assertion enforces exact creator and stateful blocker contracts', () => {
  const creatorDecision = decision('capability_gap', {
    blockers: [blocker('instance_creator_teardown_authorization', 'unavailable')],
  });
  const creatorMessage = 'I preserved the request, but did not compile it because these required capabilities are not currently supported: Creator-only room teardown authorization (unavailable). I did not build a partial or weakened version.';
  const creatorExpected = context({
    caseId: 'intent_creator_only_close_gap',
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'capability_gap',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
    expectedBlockers: 'instance_creator_teardown_authorization|unavailable|',
  });
  const creator = routedDocument(creatorDecision, creatorMessage, 'creator-close');
  assert.equal(checks.intentAdjudicationDecision(creator, creatorExpected).pass, true);

  const statefulBlockers = [
    blocker('durable_timer', 'unavailable'),
    blocker(
      'event_time_llm_decision',
      'forbidden_policy',
      'event_time_llm_execution_forbidden_v1',
    ),
    blocker('persistent_economy_ledger', 'unavailable'),
    blocker('restart_persistent_state', 'unavailable'),
  ];
  const statefulDecision = decision('capability_gap', { blockers: statefulBlockers });
  const statefulMessage = 'I preserved the request, but did not compile it because these required capabilities are not currently supported: Durable timers (unavailable), Event-time LLM decisions (forbidden by policy), Persistent economy ledger (unavailable), State preserved across restarts (unavailable). I did not build a partial or weakened version.';
  const statefulExpected = context({
    caseId: 'intent_stateful_game_gap',
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'capability_gap',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
    expectedBlockers: 'durable_timer|unavailable|,event_time_llm_decision|forbidden_policy|event_time_llm_execution_forbidden_v1,persistent_economy_ledger|unavailable|,restart_persistent_state|unavailable|',
  });
  const stateful = routedDocument(statefulDecision, statefulMessage, 'stateful-game');
  assert.equal(checks.intentAdjudicationDecision(stateful, statefulExpected).pass, true);

  const missing = JSON.parse(stateful);
  missing.turns[0].route_decision.blockers.pop();
  missing.final_intent.route_decision = missing.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(missing), statefulExpected).reason,
    /blockers=/,
  );
});

test('adjudication assertion preserves exact unclassified capability evidence', () => {
  const unknown = blocker('unclassified_intent_requirement', 'unclassified');
  unknown.evidence = [evidence(
    'intent.core.unclassified_requirements.0',
    'external consensus lease',
  )];
  const routeDecision = decision('capability_gap', {
    blockers: [unknown],
    unclassified_requirements: ['external consensus lease'],
  });
  const message = 'I preserved the request, but did not compile it because these required capabilities are not currently supported: Unclassified hard requirement (unclassified): external consensus lease. I did not build a partial or weakened version.';
  const expected = context({
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'capability_gap',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
    expectedBlockers: 'unclassified_intent_requirement|unclassified|',
    expectedUnclassifiedRequirements: 'external consensus lease',
  });
  const document = routedDocument(routeDecision, message, 'external-capability');
  assert.equal(checks.intentAdjudicationDecision(document, expected).pass, true);

  const changed = JSON.parse(document);
  changed.turns[0].route_decision.unclassified_requirements = ['different lease'];
  changed.final_intent.route_decision = changed.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(changed), expected).reason,
    /unclassified=/,
  );
});

test('adjudication assertion enforces exact live-mutation and secret boundary sets', () => {
  const cases = [
    {
      id: 'intent_reject_live_mutation',
      boundaries: ['bypass_validation_preview_approval', 'direct_live_mutation'],
      message: 'I can help with a safe design, but cannot cross these requested safety boundaries: Bypass validation, preview, and approval, Direct live mutation. Validation, preview, user approval, and secret protection remain enforced.',
    },
    {
      id: 'intent_reject_secret_disclosure',
      boundaries: ['direct_live_mutation', 'secret_disclosure'],
      message: 'I can help with a safe design, but cannot cross these requested safety boundaries: Direct live mutation, Secret disclosure. Validation, preview, user approval, and secret protection remain enforced.',
    },
  ];
  for (const fixture of cases) {
    const routeDecision = decision('reject', {
      boundary_violations: fixture.boundaries.map(violation),
    });
    const expected = context({
      caseId: fixture.id,
      expectedOutcomes: 'routed',
      expectedStagePath: 'empty>empty',
      expectedRoutePath: 'reject',
      expectedFinalStatus: 'empty',
      expectedCompiledOperations: undefined,
      completeRequest: false,
      expectedBoundaryViolations: fixture.boundaries.join(','),
    });
    const document = routedDocument(routeDecision, fixture.message, fixture.id);
    assert.equal(checks.intentAdjudicationDecision(document, expected).pass, true);

    const missing = JSON.parse(document);
    missing.turns[0].route_decision.boundary_violations.pop();
    missing.final_intent.route_decision = missing.turns[0].route_decision;
    assert.match(
      checks.intentAdjudicationDecision(JSON.stringify(missing), expected).reason,
      /boundaries=/,
    );
  }
});

test('route decision parser rejects source, manifest digest, and shape drift', () => {
  const source = JSON.parse(report());
  source.turns[0].route_decision.decision_source = 'model';
  assert.match(checks.intentReceipt(JSON.stringify(source), context()).reason, /adjudicator identity/);

  const digest = JSON.parse(report());
  digest.final_intent.route_decision.manifest_digest = '0'.repeat(64);
  assert.match(checks.intentReceipt(JSON.stringify(digest), context()).reason, /manifest identity/);

  const shape = JSON.parse(report());
  shape.turns[0].route_decision.extra = true;
  assert.match(checks.intentReceipt(JSON.stringify(shape), context()).reason, /invalid fields/);
});

test('terminal responses and final decisions cannot diverge from deterministic evidence', () => {
  const routeDecision = decision('typed_planner');
  const expected = context({
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'typed_planner',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
  });
  const message = 'I routed this supported custom static automation to the typed planner. No live system was changed.';
  const document = routedDocument(routeDecision, message, 'custom-feedback');
  assert.equal(checks.intentAdjudicationDecision(document, expected).pass, true);

  const promised = JSON.parse(document);
  promised.turns[0].message = 'I deployed the automation.';
  promised.message = promised.turns[0].message;
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(promised), expected).reason,
    /non-deterministic terminal response/,
  );

  const divergent = JSON.parse(document);
  divergent.final_intent.route_decision.adjudication_digest = 'c'.repeat(64);
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(divergent), expected).reason,
    /last reported decision/,
  );
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
