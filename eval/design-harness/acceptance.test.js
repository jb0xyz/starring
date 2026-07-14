const test = require('node:test');
const assert = require('node:assert/strict');

const { assess } = require('./acceptance');

const MANIFEST_DIGEST = '68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53';

function routeDecision(kind = 'private_study_room', overrides = {}) {
  return {
    kind,
    decision_source: 'deterministic_intent_adjudicator',
    adjudicator_version: 1,
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

function evidence(id) {
  return [{ semantic_path: `intent.${id}`, description: `Requests ${id}` }];
}

function fallbackDecision(caseId, route) {
  if (caseId === 'intent_creator_only_close_gap') {
    return routeDecision(route, {
      blockers: [{
        id: 'instance_creator_teardown_authorization',
        status: 'unavailable',
        policy_id: null,
        evidence: evidence('instance_creator_teardown_authorization'),
      }],
    });
  }
  if (caseId === 'intent_stateful_game_gap') {
    return routeDecision(route, {
      blockers: [
        ['durable_timer', 'unavailable', null],
        ['event_time_llm_decision', 'forbidden_policy', 'event_time_llm_execution_forbidden_v1'],
        ['persistent_economy_ledger', 'unavailable', null],
        ['restart_persistent_state', 'unavailable', null],
      ].map(([id, status, policy_id]) => ({ id, status, policy_id, evidence: evidence(id) })),
    });
  }
  if (caseId === 'intent_reject_live_mutation') {
    return routeDecision(route, {
      boundary_violations: ['bypass_validation_preview_approval', 'direct_live_mutation']
        .map((id) => ({ id, evidence: evidence(id) })),
    });
  }
  if (caseId === 'intent_reject_secret_disclosure') {
    return routeDecision(route, {
      boundary_violations: ['direct_live_mutation', 'secret_disclosure']
        .map((id) => ({ id, evidence: evidence(id) })),
    });
  }
  return routeDecision(route);
}

function intentCounters(overrides = {}) {
  return {
    route_calls: 1,
    proposal_acceptances: 1,
    resolution_acceptances: 0,
    compile_attempts: 1,
    compile_successes: 1,
    commits: 1,
    rollbacks: 0,
    conflicts: 0,
    stale_revision_rejections: 0,
    extraction_failures: 0,
    fallback_routes: {},
    ...overrides,
  };
}

function buildTurn() {
  return {
    id: 'build',
    input: 'Build it',
    outcome: 'ready',
    completed: true,
    message: 'Ready',
    question: null,
    halt_code: null,
    last_error: null,
    stage_before: 'empty',
    stage_after: 'preview_ready',
    intent_revision_before: 0,
    intent_revision_after: 1,
    route_decision: routeDecision(),
    draft_changed: true,
    draft_revision_before: 0,
    draft_revision_after: 22,
    model_calls: 1,
    model_tool_calls: 1,
    deterministic_operations: 22,
    intent_counters: intentCounters(),
    actual_gates: { validation_current: true, simulation_current: true },
    restart_after: false,
    restart_performed: false,
    elapsed_ms: 1000,
  };
}

function report(order, inputHash, turns = [buildTurn()]) {
  return {
    schema_version: 3,
    input_schema_version: 3,
    mode: 'intent_recipe',
    requested_model: 'gemma4:12b-mlx',
    served_model: 'gemma4:12b-mlx',
    declared_context_tokens: 16384,
    context_declaration_source: 'evaluation_provider',
    gateway_context_observed_tokens: null,
    gateway_id: `sha256-${'9'.repeat(64)}`,
    provenance: {
      source_commit: 'c'.repeat(40),
      source_dirty: false,
      build_source_commit: 'c'.repeat(40),
      build_source_dirty: false,
      binary_sha256: 'd'.repeat(64),
      attestation_kind: 'local_unsigned',
      run_id: 'intent-run',
      run_order: order,
      started_at_unix_ms: order * 3000,
      ended_at_unix_ms: order * 3000 + 2000,
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
    ruleset: { version: 1, panels: [{ key: 'private_study_room' }], modals: [], rules: [] },
    actual_gates: { validation_current: true, simulation_current: true },
    observability: { model_calls: turns.length, tool_calls: turns.length },
    final_intent: {
      status: 'preview_ready',
      receipt: {
        intent_revision: turns.length,
        candidate_revision: 22,
        input_intent_hash: inputHash,
        semantic_intent_hash: '2'.repeat(64),
        compiled_plan_hash: '3'.repeat(64),
        compiled_operations: 22,
      },
      public_status: {
        status: 'preview_ready',
        receipt: {
          intent_revision: turns.length,
          candidate_revision: 22,
          input_intent_hash: inputHash,
          semantic_intent_hash: '2'.repeat(64),
          compiled_plan_hash: '3'.repeat(64),
          compiled_operations: 22,
        },
      },
      route_decision: turns[turns.length - 1].route_decision,
      binding_fingerprint: '6'.repeat(64),
    },
    persistence: {
      backend: 'sqlite_file',
      store_writes: turns.length,
      connection_reopen_count: 0,
      final_generation: turns.length,
      snapshot_schema_version: 6,
      roundtrip_verified: false,
    },
    elapsed_ms: turns.reduce((sum, turn) => sum + turn.elapsed_ms, 0),
  };
}

function row(caseId, order, options = {}) {
  const document = report(order, options.inputHash || '1'.repeat(64), options.turns);
  return {
    vars: {
      caseId,
      cohort: 'intent_recipe',
      knownRecipe: true,
      equivalenceGroup: 'private-study-room',
      ...options.vars,
    },
    success: true,
    response: { metadata: document },
  };
}

function passingDocument() {
  const rows = [];
  let order = 1;
  const decisionTurns = (restart) => {
    const pending = {
      id: 'request',
      input: 'Build it',
      outcome: 'needs_input',
      completed: false,
      message: 'Which hub?',
      question: 'Which hub?',
      halt_code: null,
      last_error: null,
      stage_before: 'empty',
      stage_after: 'awaiting_decision',
      intent_revision_before: 0,
      intent_revision_after: 1,
      route_decision: routeDecision(),
      draft_changed: false,
      draft_revision_before: 0,
      draft_revision_after: 0,
      model_calls: 1,
      model_tool_calls: 1,
      deterministic_operations: 0,
      intent_counters: intentCounters({
        compile_attempts: 0,
        compile_successes: 0,
        commits: 0,
      }),
      actual_gates: { validation_current: false, simulation_current: false },
      elapsed_ms: 1000,
      restart_after: restart,
      restart_performed: restart,
    };
    const resolve = {
      ...buildTurn(),
      id: 'resolve',
      stage_before: 'awaiting_decision',
      intent_revision_before: 1,
      intent_revision_after: 2,
      intent_counters: intentCounters({
        route_calls: 0,
        proposal_acceptances: 0,
        resolution_acceptances: 1,
      }),
    };
    return [pending, resolve];
  };
  const fallbackRow = (caseId, route, currentOrder) => {
    const decision = fallbackDecision(caseId, route);
    const fallbackReport = report(currentOrder, '5'.repeat(64), [{
      id: 'fallback',
      input: 'Fallback request',
      outcome: 'routed',
      completed: false,
      message: 'Routed',
      question: null,
      halt_code: null,
      last_error: null,
      stage_before: 'empty',
      stage_after: 'empty',
      intent_revision_before: 0,
      intent_revision_after: 0,
      route_decision: decision,
      draft_changed: false,
      draft_revision_before: 0,
      draft_revision_after: 0,
      model_calls: 1,
      model_tool_calls: 1,
      deterministic_operations: 0,
      intent_counters: intentCounters({
        proposal_acceptances: 0,
        compile_attempts: 0,
        compile_successes: 0,
        commits: 0,
        fallback_routes: { [route]: 1 },
      }),
      actual_gates: { validation_current: false, simulation_current: false },
      restart_after: false,
      restart_performed: false,
      elapsed_ms: 1000,
    }]);
    fallbackReport.outcome = 'routed';
    fallbackReport.completed = false;
    fallbackReport.message = 'Routed';
    fallbackReport.draft_revision = 0;
    fallbackReport.ruleset = { version: 1, panels: [], modals: [], rules: [] };
    fallbackReport.actual_gates = { validation_current: false, simulation_current: false };
    fallbackReport.final_intent = {
      status: 'empty',
      receipt: null,
      public_status: { status: 'empty', expected_revision: 0 },
      route_decision: decision,
      binding_fingerprint: '6'.repeat(64),
    };
    return {
      vars: {
        caseId,
        cohort: 'intent_recipe',
        fallbackCase: true,
        expectedRoutePath: route,
      },
      success: true,
      response: { metadata: fallbackReport },
    };
  };
  for (let index = 0; index < 10; index += 1) {
    for (const caseId of [
      'intent_private_study_room_en',
      'intent_private_study_room_ko',
      'intent_discussion_then_build',
    ]) {
      rows.push(row(caseId, order, {
        inputHash: caseId === 'intent_private_study_room_ko' ? '7'.repeat(64) : '1'.repeat(64),
        vars: { completeRequest: true },
      }));
      order += 1;
    }
    const missing = row('intent_private_study_room_missing_hub', order, {
      inputHash: '4'.repeat(64),
      turns: decisionTurns(false),
      vars: { requiresDecision: true },
    });
    rows.push(missing);
    order += 1;
    const restart = row('intent_private_study_room_restart_pending', order, {
      inputHash: '4'.repeat(64),
      turns: decisionTurns(true),
      vars: { requiresDecision: true, expectedRestartCount: 1 },
    });
    restart.response.metadata.persistence.connection_reopen_count = 1;
    restart.response.metadata.persistence.roundtrip_verified = true;
    rows.push(restart);
    order += 1;
    for (const [caseId, route] of [
      ['intent_typed_planner_fallback', 'typed_planner'],
      ['intent_creator_only_close_gap', 'capability_gap'],
      ['intent_stateful_game_gap', 'capability_gap'],
      ['intent_reject_live_mutation', 'reject'],
      ['intent_reject_secret_disclosure', 'reject'],
    ]) {
      rows.push(fallbackRow(caseId, route, order));
      order += 1;
    }
  }
  return { results: { results: rows } };
}

test('checkpoint acceptance enforces repeated Gemma recipe quality and equivalence', () => {
  const assessment = assess(passingDocument());

  assert.equal(assessment.pass, true);
  assert.equal(assessment.samples, 100);
  assert.equal(assessment.p50_preview_turn_ms, 1000);
  assert.equal(assessment.p95_preview_turn_ms, 1000);
  assert.equal(assessment.equivalence_groups[0].pass, true);
  assert.equal(assessment.equivalence_groups[0].one_shot_multi_input_hashes_differ, true);
});

test('checkpoint boundary canonicalizes session configuration key order', () => {
  const document = passingDocument();
  document.results.results[0].response.metadata.session_config = {
    context_char_budget: 44000,
    max_gate_failures: 4,
    max_model_calls: 12,
    max_tool_calls: 24,
  };
  const assessment = assess(document);

  assert.equal(assessment.pass, true);
  assert.equal(assessment.checks.find((entry) => entry.name === 'single_cohort_boundary').pass, true);
});

test('mixed semantics, models, failed assertions, or missing cases cannot pass the checkpoint', () => {
  const semantics = passingDocument();
  semantics.results.results[0].response.metadata.final_intent.receipt.semantic_intent_hash = '8'.repeat(64);
  const semanticAssessment = assess(semantics);
  assert.equal(semanticAssessment.pass, false);
  assert.equal(
    semanticAssessment.checks.find((entry) => entry.name === 'semantic_plan_ruleset_equivalence').pass,
    false,
  );

  const model = passingDocument();
  model.results.results[0].response.metadata.served_model = 'other';
  assert.equal(assess(model).checks.find((entry) => entry.name === 'gemma4_only').pass, false);

  const samples = passingDocument();
  samples.results.results = samples.results.results.filter(
    (entry) => entry.vars.caseId !== 'intent_private_study_room_ko',
  );
  assert.equal(assess(samples).checks.find((entry) => entry.name === 'exact_case_manifest').pass, false);

  const assertion = passingDocument();
  assertion.results.results[0].success = false;
  assert.equal(
    assess(assertion).checks.find((entry) => entry.name === 'all_promptfoo_assertions_pass').pass,
    false,
  );
});
