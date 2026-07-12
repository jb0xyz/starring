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
  wrongVersion.schema_version = 2;
  assert.match(checks.draftShape(JSON.stringify(wrongVersion), context()).reason, /unsupported schema_version 2/);
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
