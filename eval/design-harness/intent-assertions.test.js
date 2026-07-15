const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { createRequire } = require('node:module');
const path = require('node:path');
const vm = require('node:vm');
const { pathToFileURL } = require('node:url');

const checks = require('./intent-assertions');

const MANIFEST_DIGEST = '68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53';
const REGISTRY_DIGEST = 'c78abf3510ca30c762d6377406a89e757a64f0b2485fe55d7be36c86a98341ab';
const RUNTIME_EVIDENCE = {
  durable_timer: ['intent.core.runtime_requirements.timers', 'durable'],
  event_time_llm_decision: ['intent.core.runtime_requirements.event_time_llm', 'true'],
  persistent_economy_ledger: [
    'intent.core.runtime_requirements.economy',
    'persistent_ledger',
  ],
  restart_persistent_state: [
    'intent.core.runtime_requirements.persistence',
    'restart_persistent',
  ],
};

function evidence(semanticPath = 'intent.automation_kind', description = 'Requested automation') {
  return { semantic_path: semanticPath, description };
}

function blocker(id, status, policyId = null) {
  const runtimeEvidence = RUNTIME_EVIDENCE[id];
  return {
    id,
    status,
    policy_id: policyId,
    evidence: [runtimeEvidence
      ? evidence(runtimeEvidence[0], runtimeEvidence[1])
      : evidence(`intent.runtime_requirements.${id}`, `Requires ${id}`)],
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
    adjudicator_version: 3,
    semantic_ir_digest: 'a'.repeat(64),
    request_evidence_hash: 'c'.repeat(64),
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

function staleGates() {
  return {
    validated_revision: null,
    simulated_revision: null,
    validation_current: false,
    simulation_current: false,
  };
}

function metric(frontierName = 'interpret_intent_core', callSequence = 1) {
  const detail = frontierName === 'extract_private_study_room_details';
  return {
    call_sequence: callSequence,
    attempt: 1,
    frontier_name: frontierName,
    outcome: 'succeeded',
    http_status: 200,
    served_model: 'gemma4:12b-mlx',
    request_body_bytes: detail ? 4500 : 6500,
    message_bytes: 1200,
    tool_bytes: detail ? 1600 : 1300,
    duplicated_schema_bytes: detail ? 1200 : 1100,
    prompt_tokens: 800,
    completion_tokens: 120,
    request_duration_ms: detail ? 350 : 450,
    gateway_model_duration_ms: null,
  };
}

function observability(modelCalls = 1, toolCalls = 1) {
  return {
    model_calls: modelCalls,
    tool_calls: toolCalls,
    distinct_mutation_tools: [],
    mutation_tool_calls: {},
    clarification_count: 0,
    validation_failures: 0,
    simulation_failures: 0,
    failure_signatures: {},
    repeated_errors: 0,
    repair_attempts: 0,
    repair_successes: 0,
    repair_failures: 0,
    repair_escalations: 0,
    nudge_count: 0,
    plan_submissions: 0,
    plan_acceptances: 0,
    planned_requirements: 0,
    plan_compiled_tool_calls: 0,
    plan_execution_failures: 0,
    plan_rollbacks: 0,
    plan_commits: 0,
    plan_conflicts: 0,
    intent_route_calls: 0,
    intent_proposal_acceptances: 0,
    intent_resolution_acceptances: 0,
    intent_compile_attempts: 0,
    intent_compile_successes: 0,
    intent_commits: 0,
    intent_rollbacks: 0,
    intent_conflicts: 0,
    intent_stale_revision_rejections: 0,
    intent_extraction_failures: 0,
    intent_fallback_routes: {},
    intent_compiled_operations: 0,
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
    burst_elapsed_ms: 900,
    elapsed_ms: 1000,
    model_call_metrics: [metric()],
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
    actual_gates: {
      validated_revision: 22,
      simulated_revision: 22,
      validation_current: true,
      simulation_current: true,
    },
    restart_after: false,
    restart_performed: false,
    ...overrides,
  };
}

function receipt() {
  return {
    identity_revision: 2,
    intent_revision: 1,
    candidate_revision: 22,
    request_evidence_hash: 'c'.repeat(64),
    request_evidence_entries: 1,
    compiler_input_hash: '1'.repeat(64),
    semantic_intent_hash: '2'.repeat(64),
    compiled_plan_hash: '3'.repeat(64),
    candidate_ruleset_hash: '5'.repeat(64),
    candidate_draft_hash: '6'.repeat(64),
    compiled_operations: 22,
  };
}

function report(overrides = {}) {
  const turns = overrides.turns || [turn()];
  const document = {
    schema_version: 5,
    input_schema_version: 3,
    mode: 'intent_recipe',
    intent_protocol_version: 4,
    intent_adjudicator_version: 3,
    intent_identity_revision: 2,
    requested_model: 'gemma4:12b-mlx',
    served_model: 'gemma4:12b-mlx',
    gateway_id: `sha256-${'9'.repeat(64)}`,
    declared_context_tokens: 16384,
    context_declaration_source: 'evaluation_provider',
    gateway_context_observed_tokens: null,
    catalog_identity: {
      recipe_id: 'starring.private_study_room',
      recipe_version: 1,
      extractor_revision: 8,
      normalizer_revision: 4,
      compiler_revision: 1,
      simulator_revision: 1,
      registry_digest: REGISTRY_DIGEST,
    },
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
    model_call_metrics: turns.flatMap((entry) => entry.model_call_metrics),
    draft_revision: 22,
    ruleset: { version: 1, panels: [], modals: [], rules: [] },
    actual_gates: {
      validated_revision: 22,
      simulated_revision: 22,
      validation_current: true,
      simulation_current: true,
    },
    observability: observability(),
    final_intent: {
      status: 'preview_ready',
      receipt: receipt(),
      public_status: {
        status: 'preview_ready',
        root_draft_revision: 0,
        workspace_revision: 1,
        receipt: receipt(),
      },
      route_decision: turns[turns.length - 1].route_decision,
      binding_fingerprint: '4'.repeat(64),
    },
    persistence: {
      backend: 'sqlite_file',
      store_writes: 1,
      connection_reopen_count: 0,
      final_generation: 1,
      snapshot_schema_version: 7,
      roundtrip_verified: false,
    },
    elapsed_ms: 1000,
    ...overrides,
  };
  if (document.final_intent.status === 'preview_ready') {
    const hashes = checks.candidateIdentityHashes(document);
    Object.assign(document.final_intent.receipt, hashes);
    Object.assign(document.final_intent.public_status.receipt, hashes);
  }
  document.model_call_metrics.forEach((entry, index) => {
    entry.call_sequence = index + 1;
  });
  return JSON.stringify(document);
}

function refreshCandidateHashes(document) {
  const hashes = checks.candidateIdentityHashes(document);
  Object.assign(document.final_intent.receipt, hashes);
  Object.assign(document.final_intent.public_status.receipt, hashes);
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
      ...overrides,
    },
  };
}

function routedDocument(routeDecision, message, id = 'routed', input = 'Route this request') {
  const routed = turn({
    id,
    input,
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
    actual_gates: staleGates(),
  });
  return report({
    outcome: 'routed',
    completed: false,
    message,
    turns: [routed],
    draft_revision: 0,
    actual_gates: staleGates(),
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

test('served model provenance is independent of tool calls and retry attempts stay observable', () => {
  const textResponse = JSON.parse(report());
  textResponse.turns[0].model_tool_calls = 0;
  textResponse.observability.tool_calls = 0;
  assert.equal(
    checks.intentProvenance(JSON.stringify(textResponse), context()).pass,
    true,
  );

  const retry = JSON.parse(report());
  const failed = retry.turns[0].model_call_metrics[0];
  failed.outcome = 'transport_error';
  failed.http_status = null;
  failed.served_model = null;
  failed.prompt_tokens = null;
  failed.completion_tokens = null;
  failed.request_duration_ms = 100;
  const succeeded = metric();
  succeeded.attempt = 2;
  retry.turns[0].model_call_metrics.push(succeeded);
  retry.model_call_metrics = structuredClone(retry.turns[0].model_call_metrics);

  assert.equal(checks.intentReceipt(JSON.stringify(retry), context()).pass, true);
});

test('detail-path assertions require two calls and pin every custom value to its RuleSet path', () => {
  const custom = JSON.parse(report());
  custom.turns[0].model_calls = 2;
  custom.turns[0].model_tool_calls = 2;
  const detailMetric = metric('extract_private_study_room_details', 2);
  custom.turns[0].model_call_metrics.push(detailMetric);
  custom.model_call_metrics.push(detailMetric);
  custom.observability.model_calls = 2;
  custom.observability.tool_calls = 2;
  custom.ruleset = {
    version: 1,
    panels: [{ buttons: [{ label: 'Start focus room' }] }],
    modals: [],
    rules: [
      {},
      {
        actions: [
          {},
          { name: '${input.room_name} members' },
          { name: 'focus-${input.room_name}' },
          {},
          {},
          {},
          { buttons: [{ label: 'Guide' }] },
          { buttons: [{ label: 'Join' }] },
        ],
      },
      { actions: [{ content: 'Read this first' }] },
      { actions: [{}, {}, { content: 'Joined the study room' }] },
    ],
  };
  refreshCandidateHashes(custom);
  const expected = context({
    expectedModelCallsPerTurn: '2',
    expectedToolCallsPerTurn: '2',
    expectedRulesetPathValues: JSON.stringify({
      '/panels/0/buttons/0/label': 'Start focus room',
      '/rules/1/actions/1/name': '${input.room_name} members',
      '/rules/1/actions/2/name': 'focus-${input.room_name}',
      '/rules/1/actions/6/buttons/0/label': 'Guide',
      '/rules/1/actions/7/buttons/0/label': 'Join',
      '/rules/2/actions/0/content': 'Read this first',
      '/rules/3/actions/2/content': 'Joined the study room',
    }),
    expectedRulesetAbsentPaths: JSON.stringify([
      '/rules/1/actions/6/buttons/1',
      '/rules/4',
    ]),
  });
  const document = JSON.stringify(custom);
  assert.equal(checks.intentOneCallTurns(document, expected).pass, true);
  assert.equal(checks.intentRulesetPathValues(document, expected).pass, true);

  custom.ruleset.rules[1].actions[6].buttons[0].label = 'Read this first';
  custom.ruleset.rules[2].actions[0].content = 'Guide';
  refreshCandidateHashes(custom);
  assert.match(
    checks.intentRulesetPathValues(JSON.stringify(custom), expected).reason,
    /path=\/rules\/1\/actions\/6\/buttons\/0\/label.*path=\/rules\/2\/actions\/0\/content/,
  );
});

test('report observability is strict and sequential request durations fit inside the burst', () => {
  const extra = JSON.parse(report());
  extra.observability.unknown_counter = 0;
  assert.match(
    checks.intentReceipt(JSON.stringify(extra), context()).reason,
    /observability has invalid fields/,
  );

  const missing = JSON.parse(report());
  delete missing.observability.intent_compiled_operations;
  assert.match(
    checks.intentReceipt(JSON.stringify(missing), context()).reason,
    /observability has invalid fields/,
  );

  const invalidMap = JSON.parse(report());
  invalidMap.observability.intent_fallback_routes = [];
  assert.match(
    checks.intentReceipt(JSON.stringify(invalidMap), context()).reason,
    /missing object observability.intent_fallback_routes/,
  );

  const sequential = JSON.parse(report());
  sequential.turns[0].model_calls = 2;
  sequential.turns[0].model_tool_calls = 2;
  sequential.turns[0].model_call_metrics.push(metric('extract_private_study_room_details', 2));
  sequential.model_call_metrics = sequential.turns[0].model_call_metrics;
  sequential.observability.model_calls = 2;
  sequential.observability.tool_calls = 2;
  assert.equal(
    checks.intentReceipt(JSON.stringify(sequential), context()).pass,
    true,
  );

  sequential.turns[0].model_call_metrics[0].request_duration_ms = 500;
  sequential.turns[0].model_call_metrics[1].request_duration_ms = 500;
  assert.match(
    checks.intentReceipt(JSON.stringify(sequential), context()).reason,
    /request duration exceeds burst duration/,
  );
});

test('detail-path assertion pins custom copy and untouched naming and control defaults', () => {
  const custom = JSON.parse(report());
  custom.ruleset = {
    version: 1,
    panels: [{ buttons: [{ label: 'Begin deep work' }] }],
    modals: [],
    rules: [
      {},
      {
        actions: [
          {},
          { name: '${input.room_name} members' },
          { name: 'study-${input.room_name}' },
          {},
          {},
          {},
          { buttons: [{ label: 'Help' }] },
          { buttons: [{ label: 'Join' }] },
        ],
      },
      { actions: [{ content: 'This is a private study room' }] },
      { actions: [{}, {}, { content: 'Joined the study room' }] },
    ],
  };
  refreshCandidateHashes(custom);
  const expected = context({
    expectedRulesetPathValues: JSON.stringify({
      '/panels/0/buttons/0/label': 'Begin deep work',
      '/rules/1/actions/1/name': '${input.room_name} members',
      '/rules/1/actions/2/name': 'study-${input.room_name}',
      '/rules/1/actions/6/buttons/0/label': 'Help',
      '/rules/1/actions/7/buttons/0/label': 'Join',
      '/rules/2/actions/0/content': 'This is a private study room',
      '/rules/3/actions/2/content': 'Joined the study room',
    }),
    expectedRulesetAbsentPaths: JSON.stringify([
      '/rules/1/actions/6/buttons/1',
      '/rules/4',
    ]),
  });
  assert.equal(checks.intentRulesetPathValues(JSON.stringify(custom), expected).pass, true);

  custom.ruleset.rules[1].actions[2].name = 'focus-${input.room_name}';
  refreshCandidateHashes(custom);
  assert.match(
    checks.intentRulesetPathValues(JSON.stringify(custom), expected).reason,
    /path=\/rules\/1\/actions\/2\/name/,
  );
  custom.ruleset.rules[1].actions[2].name = 'study-${input.room_name}';
  custom.ruleset.rules[1].actions[6].buttons.push({ label: 'Close' });
  refreshCandidateHashes(custom);
  assert.match(
    checks.intentRulesetPathValues(JSON.stringify(custom), expected).reason,
    /unexpected RuleSet path=\/rules\/1\/actions\/6\/buttons\/1/,
  );
});

test('Korean RuleSet expectations reject an English default RuleSet', () => {
  const english = JSON.parse(report());
  english.ruleset = {
    version: 1,
    panels: [{ content: 'Create a study room', buttons: [{ label: 'Create room' }] }],
    modals: [{ title: 'Create study room', fields: [{ label: 'Room name' }] }],
    rules: [
      {},
      {
        actions: [
          {},
          { name: '${input.room_name} members' },
          { name: 'study-${input.room_name}' },
          {},
          {},
          {},
          { content: 'Welcome to ${input.room_name}', buttons: [{ label: 'Help' }] },
          { content: '${input.room_name} is open', buttons: [{ label: 'Join' }] },
          {},
          { content: 'Created ${input.room_name}' },
        ],
      },
      { actions: [{ content: 'This is a private study room' }] },
      { actions: [{}, {}, { content: 'Joined the study room' }] },
    ],
  };
  refreshCandidateHashes(english);
  const korean = context({
    expectedRulesetPathValues: JSON.stringify({
      '/panels/0/content': '스터디룸을 만들어보세요',
      '/panels/0/buttons/0/label': '스터디룸 만들기',
      '/modals/0/title': '스터디룸 만들기',
      '/modals/0/fields/0/label': '방 이름',
      '/rules/1/actions/1/name': '${input.room_name} 멤버',
      '/rules/1/actions/6/buttons/0/label': '도움말',
      '/rules/1/actions/7/buttons/0/label': '참가하기',
      '/rules/2/actions/0/content': '멤버 역할이 있는 사용자만 볼 수 있는 비공개 스터디룸입니다',
      '/rules/3/actions/2/content': '스터디룸에 참가했습니다',
    }),
  });

  const result = checks.intentRulesetPathValues(JSON.stringify(english), korean);
  assert.equal(result.pass, false);
  assert.match(result.reason, /path=\/panels\/0\/content/);
  assert.match(result.reason, /path=\/rules\/3\/actions\/2\/content/);
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
    candidate_draft_hash: value.candidate_draft_hash,
    candidate_ruleset_hash: value.candidate_ruleset_hash,
    compiled_plan_hash: value.compiled_plan_hash,
    semantic_intent_hash: value.semantic_intent_hash,
    compiler_input_hash: value.compiler_input_hash,
    request_evidence_entries: value.request_evidence_entries,
    request_evidence_hash: value.request_evidence_hash,
    candidate_revision: value.candidate_revision,
    intent_revision: value.intent_revision,
    identity_revision: value.identity_revision,
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
    candidate_draft_hash: value.candidate_draft_hash,
    candidate_ruleset_hash: value.candidate_ruleset_hash,
    compiled_plan_hash: value.compiled_plan_hash,
    semantic_intent_hash: value.semantic_intent_hash,
    compiler_input_hash: value.compiler_input_hash,
    request_evidence_entries: value.request_evidence_entries,
    request_evidence_hash: value.request_evidence_hash,
    candidate_revision: value.candidate_revision,
    intent_revision: value.intent_revision,
    identity_revision: value.identity_revision,
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
    actual_gates: staleGates(),
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
    observability: observability(2, 2),
    persistence: {
      backend: 'sqlite_file',
      store_writes: 2,
      connection_reopen_count: 1,
      final_generation: 2,
      snapshot_schema_version: 7,
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
    actual_gates: staleGates(),
  });
  const document = report({
    outcome: 'routed',
    completed: false,
    turns: [routed],
    draft_revision: 0,
    actual_gates: staleGates(),
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

  const halted = JSON.parse(document);
  halted.turns[0].outcome = 'halted';
  const haltedResult = checks.intentNoMutationFallback(JSON.stringify(halted), expected);
  assert.equal(haltedResult.pass, false);
  assert.match(haltedResult.reason, /did not reach a deterministic terminal route/);
  assert.doesNotMatch(haltedResult.reason, /mutated or compiled/);
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
  const statefulRequirements = [
    'an LLM decides rewards at event time',
    'every message earns XP',
    'levels unlock an economy',
    'timers advance quests',
  ];
  const statefulUnclassified = blocker('unclassified_intent_requirement', 'unclassified');
  statefulUnclassified.evidence = statefulRequirements.map((description, index) => evidence(
    `intent.core.unclassified_requirements.${index}`,
    description,
  ));
  statefulBlockers.push(statefulUnclassified);
  const statefulDecision = decision('capability_gap', {
    blockers: statefulBlockers,
    unclassified_requirements: statefulRequirements,
  });
  const statefulMessage = `I preserved the request, but did not compile it because these required capabilities are not currently supported: Durable timers (unavailable), Event-time LLM decisions (forbidden by policy), Persistent economy ledger (unavailable), State preserved across restarts (unavailable), Unclassified hard requirement (unclassified): ${statefulRequirements.join(', ')}. I did not build a partial or weakened version.`;
  const statefulExpected = context({
    caseId: 'intent_stateful_game_gap',
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'capability_gap',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
    expectedBlockers: 'durable_timer|unavailable|,event_time_llm_decision|forbidden_policy|event_time_llm_execution_forbidden_v1,persistent_economy_ledger|unavailable|,restart_persistent_state|unavailable|,unclassified_intent_requirement|unclassified|',
    expectedUnclassifiedRequirements: JSON.stringify(statefulRequirements),
  });
  const statefulInput = `Build a stateful game where ${statefulRequirements.join(', ')}. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts.`;
  const stateful = routedDocument(
    statefulDecision,
    statefulMessage,
    'stateful-game',
    statefulInput,
  );
  assert.equal(checks.intentAdjudicationDecision(stateful, statefulExpected).pass, true);

  const ungrounded = JSON.parse(stateful);
  ungrounded.turns[0].input = 'Build a generic stateful game';
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(ungrounded), statefulExpected).reason,
    /unclassified evidence not grounded/,
  );

  const swappedUnclassifiedPaths = JSON.parse(stateful);
  const unclassifiedEvidence =
    swappedUnclassifiedPaths.turns[0].route_decision.blockers[4].evidence;
  [unclassifiedEvidence[0].semantic_path, unclassifiedEvidence[1].semantic_path] =
    [unclassifiedEvidence[1].semantic_path, unclassifiedEvidence[0].semantic_path];
  unclassifiedEvidence.sort((left, right) => left.semantic_path.localeCompare(right.semantic_path));
  swappedUnclassifiedPaths.final_intent.route_decision =
    swappedUnclassifiedPaths.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(
      JSON.stringify(swappedUnclassifiedPaths),
      statefulExpected,
    ).reason,
    /unclassified evidence does not match indexed unclassified_requirements/,
  );

  const swappedRuntimeEvidence = JSON.parse(stateful);
  const runtimeBlockers = swappedRuntimeEvidence.turns[0].route_decision.blockers;
  [runtimeBlockers[0].evidence, runtimeBlockers[1].evidence] =
    [runtimeBlockers[1].evidence, runtimeBlockers[0].evidence];
  swappedRuntimeEvidence.final_intent.route_decision =
    swappedRuntimeEvidence.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(
      JSON.stringify(swappedRuntimeEvidence),
      statefulExpected,
    ).reason,
    /capability evidence contract/,
  );

  const missing = JSON.parse(stateful);
  missing.turns[0].route_decision.blockers.shift();
  missing.final_intent.route_decision = missing.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(missing), statefulExpected).reason,
    /blockers=/,
  );
});

test('adjudication assertion preserves exact unclassified capability evidence', () => {
  const exactRequirement =
    'a static Discord button flow that must acquire an external consensus lease before responding';
  const unknown = blocker('unclassified_intent_requirement', 'unclassified');
  unknown.evidence = [evidence(
    'intent.core.unclassified_requirements.0',
    exactRequirement,
  )];
  const routeDecision = decision('capability_gap', {
    blockers: [unknown],
    unclassified_requirements: [exactRequirement],
  });
  const message = `I preserved the request, but did not compile it because these required capabilities are not currently supported: Unclassified hard requirement (unclassified): ${exactRequirement}. I did not build a partial or weakened version.`;
  const expected = context({
    expectedOutcomes: 'routed',
    expectedStagePath: 'empty>empty',
    expectedRoutePath: 'capability_gap',
    expectedFinalStatus: 'empty',
    expectedCompiledOperations: undefined,
    completeRequest: false,
    expectedBlockers: 'unclassified_intent_requirement|unclassified|',
    expectedUnclassifiedRequirements: exactRequirement,
  });
  const document = routedDocument(
    routeDecision,
    message,
    'external-capability',
    `Build ${exactRequirement}. Preserve the external consensus lease requirement and do not replace it with a local approximation.`,
  );
  assert.equal(checks.intentAdjudicationDecision(document, expected).pass, true);

  const changed = JSON.parse(document);
  changed.turns[0].route_decision.unclassified_requirements = ['different lease'];
  changed.final_intent.route_decision = changed.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(changed), expected).reason,
    /unclassified evidence does not match indexed unclassified_requirements/,
  );

  const shortened = JSON.parse(document);
  shortened.turns[0].route_decision.unclassified_requirements = ['external consensus lease'];
  shortened.turns[0].route_decision.blockers[0].evidence[0].description =
    'external consensus lease';
  shortened.turns[0].message = 'I preserved the request, but did not compile it because these required capabilities are not currently supported: Unclassified hard requirement (unclassified): external consensus lease. I did not build a partial or weakened version.';
  shortened.message = shortened.turns[0].message;
  shortened.final_intent.route_decision = shortened.turns[0].route_decision;
  assert.match(
    checks.intentAdjudicationDecision(JSON.stringify(shortened), expected).reason,
    /unclassified=.*expected=/,
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

  const evidenceHash = JSON.parse(report());
  delete evidenceHash.turns[0].route_decision.request_evidence_hash;
  assert.match(checks.intentReceipt(JSON.stringify(evidenceHash), context()).reason, /invalid fields/);
});

test('V4 report contract rejects version, evidence, and candidate identity drift', () => {
  const protocol = JSON.parse(report());
  protocol.intent_protocol_version = 3;
  assert.match(checks.intentReceipt(JSON.stringify(protocol), context()).reason, /contract identity/);

  const adjudicator = JSON.parse(report());
  adjudicator.intent_adjudicator_version = 2;
  assert.match(checks.intentReceipt(JSON.stringify(adjudicator), context()).reason, /contract identity/);

  const oldExtractor = JSON.parse(report());
  oldExtractor.catalog_identity.extractor_revision = 7;
  assert.match(checks.intentReceipt(JSON.stringify(oldExtractor), context()).reason, /catalog identity/);

  const oldNormalizer = JSON.parse(report());
  oldNormalizer.catalog_identity.normalizer_revision = 3;
  assert.match(checks.intentReceipt(JSON.stringify(oldNormalizer), context()).reason, /catalog identity/);

  const forgedRegistry = JSON.parse(report());
  forgedRegistry.catalog_identity.registry_digest = '8'.repeat(64);
  assert.match(checks.intentReceipt(JSON.stringify(forgedRegistry), context()).reason, /catalog identity/);

  const identity = JSON.parse(report());
  identity.final_intent.receipt.identity_revision = 1;
  identity.final_intent.public_status.receipt.identity_revision = 1;
  assert.match(checks.intentReceipt(JSON.stringify(identity), context()).reason, /identity_revision/);

  const legacyCompiler = JSON.parse(report());
  for (const value of [
    legacyCompiler.final_intent.receipt,
    legacyCompiler.final_intent.public_status.receipt,
  ]) {
    value.input_intent_hash = value.compiler_input_hash;
    delete value.compiler_input_hash;
  }
  assert.match(checks.intentReceipt(JSON.stringify(legacyCompiler), context()).reason, /invalid fields/);

  const evidence = JSON.parse(report());
  evidence.final_intent.receipt.request_evidence_entries = 0;
  evidence.final_intent.public_status.receipt.request_evidence_entries = 0;
  assert.match(checks.intentReceipt(JSON.stringify(evidence), context()).reason, /must be positive/);

  const candidate = JSON.parse(report());
  candidate.final_intent.receipt.candidate_draft_hash = 'invalid';
  candidate.final_intent.public_status.receipt.candidate_draft_hash = 'invalid';
  assert.match(checks.intentReceipt(JSON.stringify(candidate), context()).reason, /candidate_draft_hash/);

  const forgedRuleset = JSON.parse(report());
  forgedRuleset.final_intent.receipt.candidate_ruleset_hash = '7'.repeat(64);
  forgedRuleset.final_intent.public_status.receipt.candidate_ruleset_hash = '7'.repeat(64);
  assert.match(
    checks.intentReceipt(JSON.stringify(forgedRuleset), context()).reason,
    /candidate_ruleset_hash does not match/,
  );

  const forgedDraft = JSON.parse(report());
  forgedDraft.final_intent.receipt.candidate_draft_hash = '7'.repeat(64);
  forgedDraft.final_intent.public_status.receipt.candidate_draft_hash = '7'.repeat(64);
  assert.match(
    checks.intentReceipt(JSON.stringify(forgedDraft), context()).reason,
    /candidate_draft_hash does not match/,
  );

  const snapshot = JSON.parse(report());
  snapshot.persistence.snapshot_schema_version = 6;
  assert.match(checks.intentReceipt(JSON.stringify(snapshot), context()).reason, /SQLite persistence/);
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
  assert.match(checks.intentReceipt(JSON.stringify(malformed), context()).reason, /must be 5/);

  const duplicatedSchemaBudget = JSON.parse(report());
  for (const value of [
    duplicatedSchemaBudget.turns[0].model_call_metrics[0],
    duplicatedSchemaBudget.model_call_metrics[0],
  ]) {
    value.duplicated_schema_bytes = 1601;
  }
  assert.match(
    checks.intentReceipt(JSON.stringify(duplicatedSchemaBudget), context()).reason,
    /exceeds the Core schema budget/,
  );

  const combinedSchemaBudget = JSON.parse(report());
  for (const value of [
    combinedSchemaBudget.turns[0].model_call_metrics[0],
    combinedSchemaBudget.model_call_metrics[0],
  ]) {
    value.tool_bytes = 2701;
  }
  assert.match(
    checks.intentReceipt(JSON.stringify(combinedSchemaBudget), context()).reason,
    /exceeds the Core schema budget/,
  );

  const invalidByteAccounting = JSON.parse(report());
  for (const value of [
    invalidByteAccounting.turns[0].model_call_metrics[0],
    invalidByteAccounting.model_call_metrics[0],
  ]) {
    value.request_body_bytes = value.message_bytes
      + value.tool_bytes
      + value.duplicated_schema_bytes;
  }
  assert.match(
    checks.intentReceipt(JSON.stringify(invalidByteAccounting), context()).reason,
    /byte accounting is invalid/,
  );

  const wrongModel = JSON.parse(report());
  wrongModel.served_model = 'other';
  assert.match(checks.intentProvenance(JSON.stringify(wrongModel), context()).reason, /served_model/);

  const oracle = JSON.parse(report());
  oracle.oracle.enabled = true;
  assert.match(checks.intentOracleIsolation(JSON.stringify(oracle), context()).reason, /oracle=/);

  const calls = JSON.parse(report());
  calls.turns[0].model_calls = 2;
  assert.match(
    checks.intentOneCallTurns(JSON.stringify(calls), context()).reason,
    /metric call count differs from model_calls/,
  );

  const attempt = JSON.parse(report());
  attempt.turns[0].model_call_metrics[0].attempt = 2;
  attempt.model_call_metrics[0].attempt = 2;
  assert.match(
    checks.intentOneCallTurns(JSON.stringify(attempt), context()).reason,
    /attempts are not contiguous/,
  );

  const fabricatedUsage = JSON.parse(report());
  for (const value of [
    fabricatedUsage.turns[0].model_call_metrics[0],
    fabricatedUsage.model_call_metrics[0],
  ]) {
    value.outcome = 'transport_error';
    value.http_status = null;
    value.served_model = null;
  }
  assert.match(
    checks.intentProvenance(JSON.stringify(fabricatedUsage), context()).reason,
    /fabricated token usage/,
  );

  const latency = JSON.parse(report());
  latency.turns[0].elapsed_ms = 60001;
  latency.elapsed_ms = 60001;
  assert.match(checks.intentHardLatency(JSON.stringify(latency), context()).reason, /exceeded/);
});
