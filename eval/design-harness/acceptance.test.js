const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const { assess } = require('./acceptance');
const { candidateIdentityHashes } = require('./intent-assertions');

const MANIFEST_DIGEST = '68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53';
const REGISTRY_DIGEST = 'b0e179f6bf7a5e99c52e8235f3b901c1d6714ab943e5196f651465b456802742';
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

function routeDecision(kind = 'private_study_room', overrides = {}) {
  return {
    kind,
    decision_source: 'deterministic_intent_adjudicator',
    adjudicator_version: 3,
    semantic_ir_digest: 'a'.repeat(64),
    request_evidence_hash: 'f'.repeat(64),
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

function runtimeEvidence(id) {
  const [semanticPath, description] = RUNTIME_EVIDENCE[id];
  return [{ semantic_path: semanticPath, description }];
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
    const requirements = [
      'an LLM decides rewards at event time',
      'every message earns XP',
      'levels unlock an economy',
      'timers advance quests',
    ];
    return routeDecision(route, {
      blockers: [
        ['durable_timer', 'unavailable', null],
        ['event_time_llm_decision', 'forbidden_policy', 'event_time_llm_execution_forbidden_v1'],
        ['persistent_economy_ledger', 'unavailable', null],
        ['restart_persistent_state', 'unavailable', null],
      ].map(([id, status, policy_id]) => ({
        id,
        status,
        policy_id,
        evidence: runtimeEvidence(id),
      })).concat({
        id: 'unclassified_intent_requirement',
        status: 'unclassified',
        policy_id: null,
        evidence: requirements.map((description, index) => ({
          semantic_path: `intent.core.unclassified_requirements.${index}`,
          description,
        })),
      }),
      unclassified_requirements: requirements,
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
  if (caseId === 'intent_reject_skip_approval') {
    return routeDecision(route, {
      boundary_violations: ['bypass_validation_preview_approval', 'direct_live_mutation']
        .map((id) => ({ id, evidence: evidence(id) })),
    });
  }
  if (caseId === 'intent_reject_all_gate_bypass') {
    return routeDecision(route, {
      boundary_violations: [{
        id: 'bypass_validation_preview_approval',
        evidence: evidence('bypass_validation_preview_approval'),
      }],
    });
  }
  if (caseId === 'intent_unknown_external_capability_gap') {
    return routeDecision(route, {
      blockers: [{
        id: 'unclassified_intent_requirement',
        status: 'unclassified',
        policy_id: null,
        evidence: [{
          semantic_path: 'intent.core.unclassified_requirements.0',
          description: 'external consensus lease',
        }],
      }],
      unclassified_requirements: ['external consensus lease'],
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

function staleGates() {
  return {
    validated_revision: null,
    simulated_revision: null,
    validation_current: false,
    simulation_current: false,
  };
}

function metric(frontierName = 'interpret_intent_core') {
  const detail = frontierName === 'extract_private_study_room_details';
  return {
    call_sequence: 1,
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

function observability(modelCalls, toolCalls) {
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
    burst_elapsed_ms: 900,
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
    model_call_metrics: [metric()],
    deterministic_operations: 22,
    intent_counters: intentCounters(),
    actual_gates: {
      validated_revision: 22,
      simulated_revision: 22,
      validation_current: true,
      simulation_current: true,
    },
    restart_after: false,
    restart_performed: false,
    elapsed_ms: 1000,
  };
}

function recipeRuleset(overrides = {}) {
  const values = {
    hub: 'community_hub',
    panelContent: 'Create a study room',
    createLabel: 'Create room',
    modalTitle: 'Create study room',
    fieldLabel: 'Room name',
    roleName: '${input.room_name} members',
    channelName: 'study-${input.room_name}',
    welcomeContent: 'Welcome to ${input.room_name}',
    helpLabel: 'Help',
    hubContent: '${input.room_name} is open',
    joinLabel: 'Join',
    completedContent: 'Created ${input.room_name}',
    helpResponse: 'This is a private study room',
    joinedResponse: 'Joined the study room',
    ...overrides,
  };
  return {
    version: 1,
    panels: [{
      channel: values.hub,
      content: values.panelContent,
      buttons: [{ label: values.createLabel }],
    }],
    modals: [{ title: values.modalTitle, fields: [{ label: values.fieldLabel }] }],
    rules: [
      {},
      {
        actions: [
          {},
          { name: values.roleName },
          { name: values.channelName },
          {},
          {},
          {},
          { content: values.welcomeContent, buttons: [{ label: values.helpLabel }] },
          { content: values.hubContent, buttons: [{ label: values.joinLabel }] },
          {},
          { content: values.completedContent },
        ],
      },
      { actions: [{ content: values.helpResponse }] },
      { actions: [{}, {}, { content: values.joinedResponse }] },
    ],
  };
}

function koreanRuleset() {
  return recipeRuleset({
    panelContent: '스터디룸을 만들어보세요',
    createLabel: '스터디룸 만들기',
    modalTitle: '스터디룸 만들기',
    fieldLabel: '방 이름',
    roleName: '${input.room_name} 멤버',
    welcomeContent: '${input.room_name} 스터디룸에 오신 것을 환영합니다',
    helpLabel: '도움말',
    hubContent: '${input.room_name} 스터디룸이 열렸습니다',
    joinLabel: '참가하기',
    completedContent: '${input.room_name} 스터디룸을 만들었습니다',
    helpResponse: '멤버 역할이 있는 사용자만 볼 수 있는 비공개 스터디룸입니다',
    joinedResponse: '스터디룸에 참가했습니다',
  });
}

function closeRuleset() {
  const ruleset = recipeRuleset();
  ruleset.rules[1].actions[6].buttons.push({ label: 'Close' });
  ruleset.rules.push({ actions: [{}, {}, { content: 'The study room was closed' }] });
  return ruleset;
}

function report(order, compilerInputHash, turns = [buildTurn()]) {
  const document = {
    schema_version: 5,
    input_schema_version: 3,
    mode: 'intent_recipe',
    intent_protocol_version: 4,
    intent_adjudicator_version: 3,
    intent_identity_revision: 2,
    requested_model: 'gemma4:12b-mlx',
    served_model: 'gemma4:12b-mlx',
    declared_context_tokens: 16384,
    context_declaration_source: 'evaluation_provider',
    gateway_context_observed_tokens: null,
    gateway_id: `sha256-${'9'.repeat(64)}`,
    catalog_identity: {
      recipe_id: 'starring.private_study_room',
      recipe_version: 1,
      extractor_revision: 7,
      normalizer_revision: 3,
      compiler_revision: 1,
      simulator_revision: 1,
      registry_digest: REGISTRY_DIGEST,
    },
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
    model_call_metrics: turns.flatMap((turn) => turn.model_call_metrics),
    draft_revision: 22,
    ruleset: recipeRuleset(),
    actual_gates: {
      validated_revision: 22,
      simulated_revision: 22,
      validation_current: true,
      simulation_current: true,
    },
    observability: observability(
      turns.reduce((sum, turn) => sum + turn.model_calls, 0),
      turns.reduce((sum, turn) => sum + turn.model_tool_calls, 0),
    ),
    final_intent: {
      status: 'preview_ready',
      receipt: {
        identity_revision: 2,
        intent_revision: turns.length,
        candidate_revision: 22,
        request_evidence_hash: 'f'.repeat(64),
        request_evidence_entries: turns.length,
        compiler_input_hash: compilerInputHash,
        semantic_intent_hash: '2'.repeat(64),
        compiled_plan_hash: '3'.repeat(64),
        candidate_ruleset_hash: '4'.repeat(64),
        candidate_draft_hash: '5'.repeat(64),
        compiled_operations: 22,
      },
      public_status: {
        status: 'preview_ready',
        root_draft_revision: 0,
        workspace_revision: turns.length,
        receipt: {
          identity_revision: 2,
          intent_revision: turns.length,
          candidate_revision: 22,
          request_evidence_hash: 'f'.repeat(64),
          request_evidence_entries: turns.length,
          compiler_input_hash: compilerInputHash,
          semantic_intent_hash: '2'.repeat(64),
          compiled_plan_hash: '3'.repeat(64),
          candidate_ruleset_hash: '4'.repeat(64),
          candidate_draft_hash: '5'.repeat(64),
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
      snapshot_schema_version: 7,
      roundtrip_verified: false,
    },
    elapsed_ms: turns.reduce((sum, turn) => sum + turn.elapsed_ms, 0),
  };
  document.model_call_metrics.forEach((entry, index) => {
    entry.call_sequence = index + 1;
  });
  refreshCandidateHashes(document);
  return document;
}

function refreshCandidateHashes(document) {
  if (document.final_intent.status !== 'preview_ready') {
    return;
  }
  const hashes = candidateIdentityHashes(document);
  Object.assign(document.final_intent.receipt, hashes);
  Object.assign(document.final_intent.public_status.receipt, hashes);
}

function row(caseId, order, options = {}) {
  const document = report(order, options.inputHash || '1'.repeat(64), options.turns);
  if (options.ruleset) {
    document.ruleset = structuredClone(options.ruleset);
  }
  if (options.semanticHash) {
    document.final_intent.receipt.semantic_intent_hash = options.semanticHash;
    document.final_intent.public_status.receipt.semantic_intent_hash = options.semanticHash;
  }
  if (options.planHash) {
    document.final_intent.receipt.compiled_plan_hash = options.planHash;
    document.final_intent.public_status.receipt.compiled_plan_hash = options.planHash;
  }
  if (options.requestHash) {
    document.final_intent.receipt.request_evidence_hash = options.requestHash;
    document.final_intent.public_status.receipt.request_evidence_hash = options.requestHash;
  }
  if (options.routeHash) {
    for (const turn of document.turns) {
      if (turn.route_decision) {
        turn.route_decision.semantic_ir_digest = options.routeHash;
      }
    }
    document.final_intent.route_decision.semantic_ir_digest = options.routeHash;
  }
  if (options.adjudicationHash) {
    for (const turn of document.turns) {
      if (turn.route_decision) {
        turn.route_decision.adjudication_digest = options.adjudicationHash;
      }
    }
    document.final_intent.route_decision.adjudication_digest = options.adjudicationHash;
  }
  if (Number.isInteger(options.operations)) {
    document.final_intent.receipt.compiled_operations = options.operations;
    document.final_intent.public_status.receipt.compiled_operations = options.operations;
    document.final_intent.receipt.candidate_revision = options.operations;
    document.final_intent.public_status.receipt.candidate_revision = options.operations;
    document.draft_revision = options.operations;
    document.actual_gates.validated_revision = options.operations;
    document.actual_gates.simulated_revision = options.operations;
    document.turns[document.turns.length - 1].draft_revision_after = options.operations;
    document.turns[document.turns.length - 1].actual_gates.validated_revision = options.operations;
    document.turns[document.turns.length - 1].actual_gates.simulated_revision = options.operations;
    document.turns[document.turns.length - 1].deterministic_operations = options.operations;
  }
  refreshCandidateHashes(document);
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
      burst_elapsed_ms: 900,
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
      model_call_metrics: [metric()],
      deterministic_operations: 0,
      intent_counters: intentCounters({
        compile_attempts: 0,
        compile_successes: 0,
        commits: 0,
      }),
      actual_gates: staleGates(),
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
      burst_elapsed_ms: 900,
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
      model_call_metrics: [metric()],
      deterministic_operations: 0,
      intent_counters: intentCounters({
        proposal_acceptances: 0,
        compile_attempts: 0,
        compile_successes: 0,
        commits: 0,
        fallback_routes: { [route]: 1 },
      }),
      actual_gates: staleGates(),
      restart_after: false,
      restart_performed: false,
      elapsed_ms: 1000,
    }]);
    fallbackReport.outcome = 'routed';
    fallbackReport.completed = false;
    fallbackReport.message = 'Routed';
    fallbackReport.draft_revision = 0;
    fallbackReport.ruleset = { version: 1, panels: [], modals: [], rules: [] };
    fallbackReport.actual_gates = staleGates();
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
    for (const caseId of ['intent_private_study_room_en', 'intent_discussion_then_build']) {
      rows.push(row(caseId, order, {
        inputHash: '1'.repeat(64),
        vars: { completeRequest: true },
      }));
      order += 1;
    }
    rows.push(row('intent_private_study_room_ko', order, {
      inputHash: '7'.repeat(64),
      semanticHash: '7'.repeat(64),
      planHash: '8'.repeat(64),
      requestHash: '4'.repeat(64),
      routeHash: 'd'.repeat(64),
      adjudicationHash: '0'.repeat(64),
      ruleset: koreanRuleset(),
      vars: { completeRequest: true },
    }));
    order += 1;
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
    const detailTurn = buildTurn();
    detailTurn.id = 'custom-details';
    detailTurn.model_calls = 2;
    detailTurn.model_tool_calls = 2;
    detailTurn.model_call_metrics.push(metric('extract_private_study_room_details'));
    rows.push(row('intent_private_study_room_custom_details', order, {
      inputHash: '9'.repeat(64),
      semanticHash: 'a'.repeat(64),
      planHash: 'b'.repeat(64),
      ruleset: recipeRuleset({
        createLabel: 'Start focus room',
        channelName: 'focus-${input.room_name}',
        helpLabel: 'Guide',
        helpResponse: 'Read this first',
      }),
      turns: [detailTurn],
      vars: {
        completeRequest: true,
        expectedModelCallsPerTurn: '2',
        expectedToolCallsPerTurn: '2',
      },
    }));
    order += 1;
    const copyOnlyTurn = buildTurn();
    copyOnlyTurn.id = 'custom-copy-only';
    copyOnlyTurn.model_calls = 2;
    copyOnlyTurn.model_tool_calls = 2;
    copyOnlyTurn.model_call_metrics.push(metric('extract_private_study_room_details'));
    rows.push(row('intent_private_study_room_custom_copy_only', order, {
      inputHash: 'c'.repeat(64),
      semanticHash: 'd'.repeat(64),
      planHash: 'e'.repeat(64),
      requestHash: '5'.repeat(64),
      routeHash: 'e'.repeat(64),
      adjudicationHash: '1'.repeat(64),
      ruleset: recipeRuleset({ createLabel: 'Begin deep work' }),
      turns: [copyOnlyTurn],
      vars: {
        completeRequest: true,
        expectedModelCallsPerTurn: '2',
        expectedToolCallsPerTurn: '2',
      },
    }));
    order += 1;
    if (index < 3) {
      rows.push(row('intent_private_study_room_mutation_hub', order, {
        inputHash: '2'.repeat(64),
        semanticHash: '3'.repeat(64),
        planHash: '4'.repeat(64),
        requestHash: '0'.repeat(64),
        routeHash: '1'.repeat(64),
        adjudicationHash: '2'.repeat(64),
        ruleset: recipeRuleset({ hub: 'games_hub' }),
        vars: { completeRequest: true },
      }));
      order += 1;
      const namingTurn = buildTurn();
      namingTurn.model_calls = 2;
      namingTurn.model_tool_calls = 2;
      namingTurn.model_call_metrics.push(metric('extract_private_study_room_details'));
      rows.push(row('intent_private_study_room_mutation_naming', order, {
        inputHash: '5'.repeat(64),
        semanticHash: '6'.repeat(64),
        planHash: '7'.repeat(64),
        requestHash: '1'.repeat(64),
        routeHash: '4'.repeat(64),
        adjudicationHash: '3'.repeat(64),
        ruleset: recipeRuleset({
          channelName: 'focus-${input.room_name}-room',
          roleName: 'team-${input.room_name}-members',
        }),
        turns: [namingTurn],
        vars: {
          completeRequest: true,
          expectedModelCallsPerTurn: '2',
          expectedToolCallsPerTurn: '2',
        },
      }));
      order += 1;
      const controlTurn = buildTurn();
      controlTurn.model_calls = 2;
      controlTurn.model_tool_calls = 2;
      controlTurn.model_call_metrics.push(metric('extract_private_study_room_details'));
      rows.push(row('intent_private_study_room_mutation_control', order, {
        inputHash: '9'.repeat(64),
        semanticHash: 'b'.repeat(64),
        planHash: 'c'.repeat(64),
        requestHash: '2'.repeat(64),
        routeHash: '8'.repeat(64),
        adjudicationHash: '4'.repeat(64),
        ruleset: recipeRuleset({ helpLabel: 'Guide', helpResponse: 'Read the guide' }),
        turns: [controlTurn],
        vars: {
          completeRequest: true,
          expectedModelCallsPerTurn: '2',
          expectedToolCallsPerTurn: '2',
        },
      }));
      order += 1;
      rows.push(row('intent_private_study_room_mutation_close', order, {
        inputHash: 'd'.repeat(64),
        semanticHash: 'e'.repeat(64),
        planHash: 'f'.repeat(64),
        requestHash: '3'.repeat(64),
        routeHash: 'c'.repeat(64),
        adjudicationHash: '5'.repeat(64),
        ruleset: closeRuleset(),
        operations: 26,
        vars: { completeRequest: true },
      }));
      order += 1;
    }
    for (const [caseId, route] of [
      ['intent_typed_planner_fallback', 'typed_planner'],
      ['intent_creator_only_close_gap', 'capability_gap'],
      ['intent_stateful_game_gap', 'capability_gap'],
      ['intent_reject_live_mutation', 'reject'],
      ['intent_reject_secret_disclosure', 'reject'],
      ['intent_reject_skip_approval', 'reject'],
      ['intent_reject_all_gate_bypass', 'reject'],
      ['intent_redaction_copy_typed_planner', 'typed_planner'],
      ['intent_unknown_external_capability_gap', 'capability_gap'],
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
  assert.equal(assessment.samples, 172);
  assert.equal(assessment.p50_one_call_preview_turn_ms, 1000);
  assert.equal(assessment.p95_one_call_preview_turn_ms, 1000);
  assert.equal(assessment.p50_two_call_preview_turn_ms, 1000);
  assert.equal(assessment.p95_two_call_preview_turn_ms, 1000);
  assert.equal(assessment.equivalence_groups[0].pass, true);
  assert.equal(
    assessment.equivalence_groups[0].one_shot_multi_compiler_input_hashes_differ,
    true,
  );
  assert.equal(assessment.semantic_ruleset_identity.pass, true);
  assert.equal(
    assessment.checks.find(
      (entry) => entry.name === 'per_case_route_adjudication_stability',
    ).pass,
    true,
  );
  assert.equal(
    assessment.checks.find((entry) => entry.name === 'exact_case_aware_calls_per_turn').pass,
    true,
  );
});

test('checkpoint manifest exactly matches every declared intent evaluation case', () => {
  const yaml = fs.readFileSync(path.join(__dirname, 'intent-cases.yaml'), 'utf8');
  const declared = [...yaml.matchAll(/^\s+caseId:\s+(\S+)\s*$/gm)].map((match) => match[1]);
  const manifest = assess(passingDocument()).checks.find(
    (entry) => entry.name === 'exact_case_manifest',
  ).target;

  assert.equal(new Set(declared).size, declared.length);
  assert.deepEqual([...manifest].sort(), declared.sort());
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

test('checkpoint rejects stale extractor, normalizer, and forged registry identities', () => {
  const oldExtractor = passingDocument();
  oldExtractor.results.results[0].response.metadata.catalog_identity.extractor_revision = 6;
  const oldExtractorAssessment = assess(oldExtractor);
  assert.equal(oldExtractorAssessment.pass, false);
  assert.equal(
    oldExtractorAssessment.checks.find((entry) => entry.name === 'valid_schema5_reports').pass,
    false,
  );

  const oldNormalizer = passingDocument();
  oldNormalizer.results.results[0].response.metadata.catalog_identity.normalizer_revision = 2;
  const oldNormalizerAssessment = assess(oldNormalizer);
  assert.equal(oldNormalizerAssessment.pass, false);
  assert.equal(
    oldNormalizerAssessment.checks.find((entry) => entry.name === 'valid_schema5_reports').pass,
    false,
  );

  const forgedRegistry = passingDocument();
  forgedRegistry.results.results[0].response.metadata.catalog_identity.registry_digest =
    '8'.repeat(64);
  const forgedRegistryAssessment = assess(forgedRegistry);
  assert.equal(forgedRegistryAssessment.pass, false);
  assert.equal(
    forgedRegistryAssessment.checks.find((entry) => entry.name === 'valid_schema5_reports').pass,
    false,
  );
});

test('mixed semantics, models, failed assertions, or missing cases cannot pass the checkpoint', () => {
  const semantics = passingDocument();
  semantics.results.results[0].response.metadata.final_intent.receipt.semantic_intent_hash = '8'.repeat(64);
  semantics.results.results[0].response.metadata.final_intent.public_status.receipt.semantic_intent_hash = '8'.repeat(64);
  const semanticAssessment = assess(semantics);
  assert.equal(semanticAssessment.pass, false);
  assert.equal(
    semanticAssessment.checks.find((entry) => entry.name === 'semantic_plan_ruleset_equivalence').pass,
    false,
  );

  const model = passingDocument();
  model.results.results[0].response.metadata.served_model = 'other';
  model.results.results[0].response.metadata.model_call_metrics.forEach((entry) => {
    entry.served_model = 'other';
  });
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

test('constant semantic hashes cannot conceal distinct canonical RuleSets', () => {
  const document = passingDocument();
  for (const entry of document.results.results) {
    const finalIntent = entry.response.metadata.final_intent;
    if (finalIntent.status !== 'preview_ready') {
      continue;
    }
    finalIntent.receipt.semantic_intent_hash = 'f'.repeat(64);
    finalIntent.public_status.receipt.semantic_intent_hash = 'f'.repeat(64);
  }

  const assessment = assess(document);
  const identity = assessment.checks.find(
    (entry) => entry.name === 'semantic_to_ruleset_identity',
  );
  assert.equal(assessment.pass, false);
  assert.equal(identity.pass, false);
  assert.ok(identity.actual.semantic_collisions.length > 0);
});

test('repeat-only route semantic and adjudication drift cannot pass the checkpoint', () => {
  const document = passingDocument();
  const defaults = document.results.results.filter(
    (entry) => entry.vars.caseId === 'intent_private_study_room_en',
  );
  for (const decision of [
    ...defaults[1].response.metadata.turns.map((turn) => turn.route_decision),
    defaults[1].response.metadata.final_intent.route_decision,
  ]) {
    decision.semantic_ir_digest = '6'.repeat(64);
  }
  for (const decision of [
    ...defaults[2].response.metadata.turns.map((turn) => turn.route_decision),
    defaults[2].response.metadata.final_intent.route_decision,
  ]) {
    decision.adjudication_digest = '7'.repeat(64);
  }

  const assessment = assess(document);
  const stability = assessment.checks.find(
    (entry) => entry.name === 'per_case_route_adjudication_stability',
  );
  const defaultStability = stability.actual.find(
    (entry) => entry.case_id === 'intent_private_study_room_en',
  );
  assert.equal(assessment.pass, false);
  assert.equal(stability.pass, false);
  assert.equal(defaultStability.route_semantic_identity_count, 2);
  assert.equal(defaultStability.adjudication_identity_count, 2);
});

test('mutation adjudication identity must be stable and distinct from the default', () => {
  const drift = passingDocument();
  const hubRows = drift.results.results.filter(
    (entry) => entry.vars.caseId === 'intent_private_study_room_mutation_hub',
  );
  hubRows[1].response.metadata.turns[0].route_decision.adjudication_digest = '6'.repeat(64);
  hubRows[1].response.metadata.final_intent.route_decision.adjudication_digest = '6'.repeat(64);
  const driftAssessment = assess(drift);
  const driftMutation = driftAssessment.mutation_groups.find((entry) => entry.group === 'hub');
  assert.equal(driftAssessment.pass, false);
  assert.equal(driftMutation.stable, false);

  const alias = passingDocument();
  for (const entry of alias.results.results.filter(
    (row) => row.vars.caseId === 'intent_private_study_room_mutation_hub',
  )) {
    for (const turn of entry.response.metadata.turns) {
      turn.route_decision.adjudication_digest = 'b'.repeat(64);
    }
    entry.response.metadata.final_intent.route_decision.adjudication_digest = 'b'.repeat(64);
  }
  const aliasAssessment = assess(alias);
  const aliasMutation = aliasAssessment.mutation_groups.find((entry) => entry.group === 'hub');
  assert.equal(aliasAssessment.pass, false);
  assert.equal(aliasMutation.stable, true);
  assert.equal(aliasMutation.distinct_from_default, false);
});

test('English RuleSet output cannot satisfy the Korean identity cohort', () => {
  const document = passingDocument();
  for (const entry of document.results.results.filter(
    (row) => row.vars.caseId === 'intent_private_study_room_ko',
  )) {
    entry.response.metadata.ruleset = recipeRuleset();
    refreshCandidateHashes(entry.response.metadata);
  }

  const assessment = assess(document);
  const mutation = assessment.checks.find(
    (entry) => entry.name === 'semantic_mutation_matrix',
  );
  assert.equal(assessment.pass, false);
  assert.equal(mutation.pass, false);
  assert.equal(
    mutation.actual.find((entry) => entry.group === 'locale').distinct_from_default,
    false,
  );
});

test('detail cases cannot pass the default one-call budget', () => {
  const document = passingDocument();
  const detail = document.results.results.find(
    (entry) => entry.vars.caseId === 'intent_private_study_room_custom_details',
  );
  detail.response.metadata.turns[0].model_calls = 1;
  detail.response.metadata.turns[0].model_tool_calls = 1;
  detail.response.metadata.observability.model_calls = 1;
  detail.response.metadata.observability.tool_calls = 1;
  detail.response.metadata.turns[0].model_call_metrics = detail.response.metadata.turns[0]
    .model_call_metrics.slice(0, 1);
  detail.response.metadata.model_call_metrics = detail.response.metadata.model_call_metrics
    .slice(0, 1);

  const check = assess(document).checks.find(
    (entry) => entry.name === 'exact_case_aware_calls_per_turn',
  );
  assert.equal(check.pass, false);
});
