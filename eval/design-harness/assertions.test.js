const test = require('node:test');
const assert = require('node:assert/strict');

const checks = require('./assertions');

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
