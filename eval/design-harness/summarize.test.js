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
