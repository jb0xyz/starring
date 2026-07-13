const test = require('node:test');
const assert = require('node:assert/strict');

const checks = require('./assertions');
const fixtures = require('./fixtures.json');

function context(overrides = {}) {
  return {
    vars: {
      requireSimulation: false,
      expectedPanels: 0,
      expectedModals: 1,
      expectedRules: 1,
      expectedActions: 2,
      minDistinctMutationTools: 3,
      maxModelCalls: 6,
      maxToolCalls: 8,
      ...overrides,
    },
  };
}

function report(overrides = {}) {
  return JSON.stringify({
    schema_version: 1,
    outcome: 'awaiting_human',
    completed: false,
    question: 'Validation passed; stop this benchmark?',
    final_validate_passed: true,
    final_simulate_passed: false,
    draft: { panels: 0, modals: 1, rules: 1, actions: 2, unresolved_references: [] },
    ruleset: {},
    observability: { model_calls: 4, tool_calls: 5, distinct_mutation_tools: ['add_modal', 'begin_rule', 'add_interaction_action'] },
    max_repeat_count: 1,
    ...overrides,
  });
}

test('validation-only cases do not require the StudyRoom simulator', () => {
  assert.equal(checks.finalGates(report(), context()).pass, true);
  assert.equal(checks.finalGates(report(), context({ requireSimulation: true })).pass, false);
  assert.equal(checks.terminalOutcome(report(), context({ caseId: 'simple_modal_ack' })).pass, true);
});

test('shape, tool diversity, repeat, and budget assertions reject regressions', () => {
  assert.equal(checks.draftShape(report(), context()).pass, true);
  assert.equal(checks.distinctMutationTools(report(), context()).pass, true);
  assert.equal(checks.noExcessiveRepeatedErrors(report({ max_repeat_count: 3 })).pass, false);
  assert.equal(checks.callBudgets(report({ observability: { model_calls: 7, tool_calls: 5, distinct_mutation_tools: [] } }), context()).pass, false);
});

test('invalid or incomplete reports fail with a schema reason', () => {
  assert.match(checks.finalGates('{', context()).reason, /^invalid eval report:/);
  const incomplete = JSON.parse(report());
  delete incomplete.final_simulate_passed;
  assert.equal(checks.finalGates(JSON.stringify(incomplete), context()).pass, false);
  assert.match(checks.finalGates(JSON.stringify(incomplete), context()).reason, /missing boolean final_simulate_passed/);
  const wrongVersion = JSON.parse(report());
  wrongVersion.schema_version = 3;
  assert.match(checks.draftShape(JSON.stringify(wrongVersion), context()).reason, /unsupported schema_version 3/);
});

function statefulReport(overrides = {}) {
  return JSON.stringify({
    schema_version: 2,
    outcome: 'ready',
    completed: true,
    question: null,
    draft: { panels: 0, modals: 1, rules: 1, actions: 2, unresolved_references: [] },
    ruleset: {},
    actual_gates: { validation_current: true, simulation_current: false },
    postcheck: {
      validate_passed: true,
      validate_error: null,
      simulate_attempted: true,
      simulate_passed: false,
      simulate_error: {},
    },
    turns: [
      {
        id: 'clarify',
        outcome: 'needs_input',
        question: 'Which format?',
        draft_revision_before: 0,
        draft_revision_after: 0,
        draft_changed: false,
        observability_delta: { model_calls: 1, tool_calls: 0 },
      },
      {
        id: 'build',
        outcome: 'ready',
        question: null,
        draft_revision_before: 0,
        draft_revision_after: 3,
        draft_changed: true,
        observability_delta: { model_calls: 3, tool_calls: 4 },
      },
    ],
    observability: { model_calls: 4, tool_calls: 4, distinct_mutation_tools: [] },
    max_repeat_count: 0,
    ...overrides,
  });
}

test('stateful assertions distinguish actual stamps and postchecks', () => {
  const statefulContext = context({
    expectedOutcomes: ['ready'],
    inputTurnCount: 2,
    firstTurnNeedsInput: true,
    minChangedTurns: 1,
    requireActualValidation: true,
    requireActualSimulation: false,
    maxModelCallsPerTurn: 4,
    maxToolCallsPerTurn: 6,
  });

  assert.equal(checks.terminalOutcome(statefulReport(), statefulContext).pass, true);
  assert.equal(checks.conversationFlow(statefulReport(), statefulContext).pass, true);
  assert.equal(checks.actualGateStamps(statefulReport(), statefulContext).pass, true);
  assert.equal(checks.finalGates(statefulReport(), statefulContext).pass, true);
  assert.equal(checks.perTurnBudgets(statefulReport(), statefulContext).pass, true);
  assert.equal(checks.actualGateStamps(
    statefulReport({ actual_gates: { validation_current: false, simulation_current: false } }),
    statefulContext,
  ).pass, false);
});

test('stateful flow detects skipped turns and revision discontinuity', () => {
  const document = JSON.parse(statefulReport());
  document.turns[1].draft_revision_before = 1;
  const statefulContext = context({ inputTurnCount: 3, firstTurnNeedsInput: true, minChangedTurns: 1 });

  const outcome = checks.conversationFlow(JSON.stringify(document), statefulContext);

  assert.equal(outcome.pass, false);
  assert.match(outcome.reason, /turns=2 expected=3/);
  assert.match(outcome.reason, /revision discontinuity/);
});

test('stateful flow enforces clarification purity and stable update tools', () => {
  const document = JSON.parse(statefulReport());
  document.turns[1].observability_delta.mutation_tool_calls = {
    update_modal: 1,
    update_action: 1,
  };
  const statefulContext = context({
    inputTurnCount: 2,
    firstTurnNeedsInput: true,
    requireDraftUnchanged: true,
    minChangedTurns: 1,
    requiredLastTurnMutationTools: ['update_modal', 'update_action'],
    forbiddenLastTurnMutationTools: ['add_modal'],
  });

  assert.equal(checks.conversationFlow(JSON.stringify(document), statefulContext).pass, true);
  document.turns[0].draft_changed = true;
  assert.match(
    checks.conversationFlow(JSON.stringify(document), statefulContext).reason,
    /clarification turn changed/,
  );
  document.turns[0].draft_changed = false;
  document.turns[1].observability_delta.mutation_tool_calls.add_modal = 1;
  assert.match(
    checks.conversationFlow(JSON.stringify(document), statefulContext).reason,
    /forbidden mutation tool add_modal/,
  );
});

test('task semantics require the exact simple RuleSet', () => {
  const ruleset = {
    version: 1,
    panels: [],
    modals: [{
      key: 'feedback_modal',
      title: 'Feedback',
      fields: [{ key: 'message', label: 'Message', style: 'paragraph', required: true }],
    }],
    rules: [{
      key: 'ack_feedback',
      trigger: { type: 'modal_submit', modal: 'feedback_modal' },
      actions: [
        { type: 'defer_ephemeral' },
        { type: 'edit_response', content: 'Thanks, ${input.message}' },
      ],
    }],
  };
  assert.equal(checks.taskSemantics(report({ ruleset }), context({ caseId: 'simple_modal_ack' })).pass, true);
  ruleset.rules[0].actions[1].content = 'wrong';
  assert.equal(checks.taskSemantics(report({ ruleset }), context({ caseId: 'simple_modal_ack' })).pass, false);
});

test('incremental StudyRoom uses the full StudyRoom semantic target', () => {
  const outcome = checks.taskSemantics(
    report({ ruleset: { version: 1, panels: [], modals: [], rules: [] } }),
    context({ caseId: 'studyroom_incremental' }),
  );

  assert.equal(outcome.pass, false);
  assert.match(outcome.reason, /studyroom_incremental ruleset does not exactly match/);
});

test('isolated resource assertions require exact semantics revisions and oracle accounting', () => {
  const document = JSON.parse(statefulReport({
    input_schema_version: 2,
    mode: 'typed_plan',
    outcome: 'progressed',
    completed: false,
    draft: { panels: 1, modals: 1, rules: 2, actions: 7, unresolved_references: [] },
    ruleset: structuredClone(fixtures.studyroom_before_finalize.ruleset),
    actual_gates: { validation_current: false, simulation_current: false },
    injected_control_calls: 2,
    delegated_model_calls: 1,
    observability: {
      model_calls: 3,
      tool_calls: 10,
      distinct_mutation_tools: [],
      plan_submissions: 1,
      plan_acceptances: 1,
      plan_commits: 1,
      plan_execution_failures: 0,
      plan_rollbacks: 0,
      plan_conflicts: 0,
    },
    turns: [{
      id: 'submit-resources',
      outcome: 'progressed',
      question: null,
      draft_revision_before: 5,
      draft_revision_after: 12,
      draft_changed: true,
      injected_control_calls: 2,
      delegated_model_calls: 1,
      observability_delta: {
        model_calls: 3,
        tool_calls: 10,
        mutation_tool_calls: {},
        plan_submissions: 1,
        plan_acceptances: 1,
        plan_commits: 1,
        plan_execution_failures: 0,
        plan_rollbacks: 0,
        plan_conflicts: 0,
      },
    }],
  }));
  const isolated = context({
    caseId: 'studyroom_resources_oracle',
    expectedOutcomes: ['progressed'],
    inputTurnCount: 1,
    minChangedTurns: 1,
    expectedInitialRevision: 5,
    expectedFinalRevision: 12,
    expectedRevisionPath: ['5>12'],
    expectedLastTurnId: 'submit-resources',
    expectedInjectedControlCalls: 2,
    expectedInjectedCallsPerTurn: ['2'],
    expectedPlanAcceptancesPerTurn: ['1'],
    expectedPlanCommitsPerTurn: ['1'],
    requireOracleProvenance: true,
    forbidActualValidation: true,
    forbidActualSimulation: true,
  });

  assert.equal(checks.taskSemantics(JSON.stringify(document), isolated).pass, true);
  assert.equal(checks.conversationFlow(JSON.stringify(document), isolated).pass, true);
  assert.equal(checks.actualGateStamps(JSON.stringify(document), isolated).pass, true);
  assert.equal(checks.oracleControlCalls(JSON.stringify(document), isolated).pass, true);

  document.ruleset.rules[1].actions[1].name = 'wrong';
  assert.equal(checks.taskSemantics(JSON.stringify(document), isolated).pass, false);
  document.ruleset = structuredClone(fixtures.studyroom_before_finalize.ruleset);
  document.turns[0].draft_revision_after = 13;
  assert.match(
    checks.conversationFlow(JSON.stringify(document), isolated).reason,
    /final_revision=13 expected=12/,
  );
  document.turns[0].draft_revision_after = 12;
  document.observability.plan_submissions = 2;
  assert.match(
    checks.oracleControlCalls(JSON.stringify(document), isolated).reason,
    /plan_submissions=2 expected=1/,
  );
  document.observability.plan_submissions = 1;
  document.turns[0].observability_delta.plan_execution_failures = 1;
  assert.match(
    checks.oracleControlCalls(JSON.stringify(document), isolated).reason,
    /submit-resources plan_execution_failures=1 expected=0/,
  );
  document.turns[0].observability_delta.plan_execution_failures = 0;
  document.delegated_model_calls = 0;
  document.turns[0].delegated_model_calls = 0;
  assert.match(
    checks.oracleControlCalls(JSON.stringify(document), isolated).reason,
    /model_calls=3 accounted=2/,
  );
});

test('typed production assertions require zero injected control calls', () => {
  const document = JSON.parse(statefulReport({
    input_schema_version: 2,
    mode: 'typed_plan',
    outcome: 'progressed',
    completed: false,
    draft: { panels: 1, modals: 1, rules: 2, actions: 7, unresolved_references: [] },
    ruleset: structuredClone(fixtures.studyroom_before_finalize.ruleset),
    actual_gates: { validation_current: false, simulation_current: false },
    injected_control_calls: 0,
    delegated_model_calls: 3,
    observability: { model_calls: 3, tool_calls: 10, distinct_mutation_tools: [] },
    turns: [{
      id: 'submit-resources',
      outcome: 'progressed',
      question: null,
      draft_revision_before: 5,
      draft_revision_after: 12,
      draft_changed: true,
      injected_control_calls: 0,
      delegated_model_calls: 3,
      observability_delta: { model_calls: 3, tool_calls: 10, mutation_tool_calls: {} },
    }],
  }));
  const production = context({
    caseId: 'studyroom_resources_typed',
    expectedOutcomes: ['progressed'],
    inputTurnCount: 1,
    minChangedTurns: 1,
    expectedInitialRevision: 5,
    expectedFinalRevision: 12,
    expectedRevisionPath: ['5>12'],
    expectedLastTurnId: 'submit-resources',
    expectedInjectedControlCalls: 0,
    expectedInjectedCallsPerTurn: ['0'],
  });

  assert.equal(checks.taskSemantics(JSON.stringify(document), production).pass, true);
  assert.equal(checks.conversationFlow(JSON.stringify(document), production).pass, true);
  assert.equal(checks.oracleControlCalls(JSON.stringify(document), production).pass, true);

  document.injected_control_calls = 1;
  document.delegated_model_calls = 2;
  document.turns[0].injected_control_calls = 1;
  assert.equal(checks.oracleControlCalls(JSON.stringify(document), production).pass, false);
});

test('five-turn oracle assertions separate nine controls from four plans', () => {
  const path = [
    ['surface', 0, 3, 2, 1],
    ['open-rule', 3, 5, 2, 1],
    ['submit-resources', 5, 12, 2, 1],
    ['submit-finalize', 12, 16, 2, 1],
    ['validate-simulate', 16, 16, 1, 0],
  ];
  const turns = path.map(([id, before, after, injected, plans]) => ({
    id,
    outcome: id === 'validate-simulate' ? 'ready' : 'progressed',
    question: null,
    draft_revision_before: before,
    draft_revision_after: after,
    draft_changed: before !== after,
    injected_control_calls: injected,
    delegated_model_calls: 1,
    observability_delta: {
      model_calls: 1 + injected,
      tool_calls: 4,
      mutation_tool_calls: {},
      plan_submissions: plans,
      plan_acceptances: plans,
      plan_commits: plans,
      plan_execution_failures: 0,
      plan_rollbacks: 0,
      plan_conflicts: 0,
    },
  }));
  const document = JSON.parse(statefulReport({
    input_schema_version: 2,
    mode: 'typed_plan',
    outcome: 'ready',
    completed: true,
    injected_control_calls: 9,
    delegated_model_calls: 5,
    observability: {
      model_calls: 14,
      tool_calls: 20,
      distinct_mutation_tools: [],
      plan_submissions: 4,
      plan_acceptances: 4,
      plan_commits: 4,
      plan_execution_failures: 0,
      plan_rollbacks: 0,
      plan_conflicts: 0,
    },
    turns,
  }));
  const incremental = context({
    inputTurnCount: 5,
    minChangedTurns: 4,
    expectedInitialRevision: 0,
    expectedFinalRevision: 16,
    expectedRevisionPath: ['0>3', '3>5', '5>12', '12>16', '16>16'],
    expectedLastTurnId: 'validate-simulate',
    expectedInjectedControlCalls: 9,
    expectedInjectedCallsPerTurn: ['2', '2', '2', '2', '1'],
    expectedPlanAcceptancesPerTurn: ['1', '1', '1', '1', '0'],
    expectedPlanCommitsPerTurn: ['1', '1', '1', '1', '0'],
    requireOracleProvenance: true,
    forbiddenLastTurnMutationTools: [
      'add_panel',
      'add_button',
      'add_modal',
      'begin_rule',
      'add_interaction_action',
      'add_resource_action',
      'add_upsert_overwrite_action',
      'add_grant_role_action',
      'add_post_panel_action',
      'set_register_instance',
    ],
  });

  assert.equal(checks.conversationFlow(JSON.stringify(document), incremental).pass, true);
  assert.equal(checks.oracleControlCalls(JSON.stringify(document), incremental).pass, true);

  document.turns[4].draft_revision_after = 17;
  assert.match(
    checks.conversationFlow(JSON.stringify(document), incremental).reason,
    /final_revision=17 expected=16/,
  );
  document.turns[4].draft_revision_after = 16;
  document.turns[4].injected_control_calls = 0;
  assert.equal(checks.oracleControlCalls(JSON.stringify(document), incremental).pass, false);
});
