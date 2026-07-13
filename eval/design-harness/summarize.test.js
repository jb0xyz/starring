const test = require('node:test');
const assert = require('node:assert/strict');

const { summarize } = require('./summarize');

test('summary keeps provider errors in the denominator and ignores missing metrics', () => {
  const validReport = {
    schema_version: 1,
    completed: true,
    final_validate_passed: true,
    final_simulate_passed: true,
    elapsed_ms: 100,
    max_repeat_count: 2,
    observability: { model_calls: 4, tool_calls: 8, distinct_mutation_tools: ['a', 'b'] },
  };
  const document = {
    results: {
      results: [
        {
          provider: { label: 'gemma' },
          vars: { caseId: 'studyroom_full', requireSimulation: true },
          success: true,
          response: { output: JSON.stringify(validReport), metadata: validReport },
        },
        {
          provider: { label: 'gemma' },
          vars: { caseId: 'studyroom_full', requireSimulation: true },
          success: false,
          response: { error: 'timeout' },
        },
        {
          provider: { label: 'gemma' },
          vars: { caseId: 'studyroom_full', requireSimulation: true },
          success: false,
          response: { output: 'not-json' },
        },
      ],
    },
  };

  const [row] = summarize(document);

  assert.equal(row.runs, 3);
  assert.equal(row.valid_reports, 1);
  assert.equal(row.valid_report_rate, 1 / 3);
  assert.equal(row.provider_error_rate, 2 / 3);
  assert.equal(row.pass_rate, 1 / 3);
  assert.equal(row.mean_elapsed_ms, 100);
  assert.equal(row.mean_model_calls, 4);
  assert.equal(row.maximum_repeat_count, 2);
  assert.equal(row.mean_repair_attempts, 0);
  assert.equal(row.mean_repair_successes, 0);
  assert.equal(row.mean_repair_failures, 0);
  assert.equal(row.mean_repair_escalations, 0);
});

test('summary aggregates repair observability', () => {
  const reports = [
    { repair_attempts: 1, repair_successes: 1, repair_failures: 0, repair_escalations: 0 },
    { repair_attempts: 1, repair_successes: 0, repair_failures: 1, repair_escalations: 0 },
    { repair_attempts: 0, repair_successes: 0, repair_failures: 0, repair_escalations: 1 },
  ];
  const document = {
    results: reports.map((observability) => ({
      provider: { label: 'gemma' },
      vars: { caseId: 'repair', requireSimulation: false },
      success: false,
      response: {
        output: JSON.stringify({
          schema_version: 1,
          elapsed_ms: 1,
          max_repeat_count: 1,
          observability: {
            model_calls: 1,
            tool_calls: 1,
            distinct_mutation_tools: [],
            ...observability,
          },
        }),
      },
    })),
  };

  const [row] = summarize(document);

  assert.equal(row.mean_repair_attempts, 2 / 3);
  assert.equal(row.mean_repair_successes, 1 / 3);
  assert.equal(row.mean_repair_failures, 1 / 3);
  assert.equal(row.mean_repair_escalations, 1 / 3);
});

test('summary aggregates stateful turn and actual gate metrics separately', () => {
  const report = {
    schema_version: 2,
    outcome: 'ready',
    completed: true,
    elapsed_ms: 50,
    max_repeat_count: 0,
    actual_gates: { validation_current: true, simulation_current: false },
    postcheck: { validate_passed: true, simulate_passed: true },
    observability: { model_calls: 4, tool_calls: 5, distinct_mutation_tools: ['a'] },
    turns: [
      {
        outcome: 'needs_input',
        draft_changed: false,
        elapsed_ms: 10,
        observability_delta: { model_calls: 1, tool_calls: 0 },
      },
      {
        outcome: 'ready',
        draft_changed: true,
        elapsed_ms: 40,
        observability_delta: { model_calls: 3, tool_calls: 5 },
      },
    ],
  };
  const document = {
    results: [{
      provider: { label: 'qwen' },
      vars: { caseId: 'stateful', requireSimulation: true },
      success: true,
      response: { metadata: report },
    }],
  };

  const [row] = summarize(document);

  assert.equal(row.validation_rate, 1);
  assert.equal(row.required_simulation_rate, 1);
  assert.equal(row.actual_validation_current_rate, 1);
  assert.equal(row.actual_simulation_current_rate, 0);
  assert.equal(row.ready_rate, 1);
  assert.equal(row.clarification_rate, 1);
  assert.equal(row.mean_turns, 2);
  assert.equal(row.changed_turn_rate, 0.5);
  assert.equal(row.needs_input_turn_rate, 0.5);
  assert.equal(row.mean_turn_elapsed_ms, 25);
  assert.equal(row.p95_turn_elapsed_ms, 40);
  assert.equal(row.mean_turn_model_calls, 2);
  assert.equal(row.mean_turn_tool_calls, 2.5);
});
