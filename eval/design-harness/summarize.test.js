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

test('summary separates oracle injections from delegated model calls and semantic assertions', () => {
  const report = {
    schema_version: 2,
    outcome: 'progressed',
    completed: false,
    elapsed_ms: 20,
    max_repeat_count: 0,
    actual_gates: { validation_current: false, simulation_current: false },
    postcheck: { validate_passed: true, simulate_passed: false },
    observability: {
      model_calls: 3,
      tool_calls: 9,
      distinct_mutation_tools: ['a'],
      plan_submissions: 1,
      plan_acceptances: 1,
      planned_requirements: 8,
      plan_compiled_tool_calls: 7,
      plan_execution_failures: 0,
      plan_rollbacks: 0,
      plan_commits: 1,
      plan_conflicts: 0,
    },
    injected_control_calls: 1,
    delegated_model_calls: 2,
    turns: [],
  };
  const document = {
    results: [{
      provider: { label: 'gemma' },
      vars: { caseId: 'oracle', requireSimulation: false },
      success: true,
      gradingResult: {
        componentResults: [{
          pass: true,
          assertion: { value: 'file://assertions.js:taskSemantics' },
        }],
      },
      response: { metadata: report },
    }],
  };

  const [row] = summarize(document);

  assert.equal(row.exact_semantics_rate, 1);
  assert.equal(row.mean_injected_control_calls, 1);
  assert.equal(row.mean_delegated_model_calls, 2);
  assert.equal(row.mean_plan_submissions, 1);
  assert.equal(row.mean_plan_acceptances, 1);
  assert.equal(row.mean_planned_requirements, 8);
  assert.equal(row.mean_plan_compiled_tool_calls, 7);
  assert.equal(row.mean_plan_execution_failures, 0);
  assert.equal(row.mean_plan_rollbacks, 0);
  assert.equal(row.mean_plan_commits, 1);
  assert.equal(row.mean_plan_conflicts, 0);
});

test('summary exposes Gemma intent cohort boundaries, isolation, operations, and latency', () => {
  const report = {
    schema_version: 5,
    outcome: 'ready',
    completed: true,
    elapsed_ms: 7000,
    requested_model: 'gemma4:12b-mlx',
    served_model: 'gemma4:12b-mlx',
    declared_context_tokens: 16384,
    context_declaration_source: 'evaluation_provider',
    gateway_context_observed_tokens: null,
    gateway_id: `sha256-${'1'.repeat(64)}`,
    provenance: {
      source_commit: 'a'.repeat(40),
      source_dirty: false,
      build_source_commit: 'a'.repeat(40),
      build_source_dirty: false,
      binary_sha256: 'b'.repeat(64),
      attestation_kind: 'local_unsigned',
      run_id: 'intent-run',
      run_order: 4,
      started_at_unix_ms: 100,
      ended_at_unix_ms: 7100,
    },
    session_config: {
      max_model_calls: 12,
      max_tool_calls: 24,
      max_gate_failures: 4,
      context_char_budget: 44000,
    },
    oracle: { enabled: false, injected_control_calls: 0 },
    actual_gates: { validation_current: true, simulation_current: true },
    final_intent: {
      status: 'preview_ready',
      receipt: { compiled_operations: 22 },
    },
    observability: { model_calls: 1, tool_calls: 1, distinct_mutation_tools: [] },
    model_call_metrics: [{
      call_sequence: 1,
      attempt: 1,
      frontier_name: 'interpret_intent_core',
      outcome: 'succeeded',
      http_status: 200,
      served_model: 'gemma4:12b-mlx',
      request_body_bytes: 6400,
      message_bytes: 1200,
      tool_bytes: 2500,
      duplicated_schema_bytes: 2000,
      prompt_tokens: 800,
      completion_tokens: 120,
      request_duration_ms: 5900,
      gateway_model_duration_ms: null,
    }],
    turns: [{
      outcome: 'ready',
      draft_changed: true,
      burst_elapsed_ms: 6000,
      elapsed_ms: 7000,
      model_calls: 1,
      model_tool_calls: 1,
      deterministic_operations: 22,
    }],
  };
  const document = {
    results: {
      results: [{
        provider: { label: 'gemma-intent' },
        vars: { caseId: 'intent', cohort: 'intent_recipe', requireSimulation: true },
        success: true,
        response: { metadata: report },
      }, {
        provider: { label: 'gemma-intent' },
        vars: { caseId: 'intent', cohort: 'intent_recipe', requireSimulation: true },
        success: true,
        response: {
          metadata: {
            ...structuredClone(report),
            session_config: {
              context_char_budget: 44000,
              max_gate_failures: 4,
              max_model_calls: 12,
              max_tool_calls: 24,
            },
          },
        },
      }],
    },
  };

  const [row] = summarize(document);

  assert.equal(row.cohort, 'intent_recipe');
  assert.equal(row.validation_rate, 1);
  assert.equal(row.required_simulation_rate, 1);
  assert.equal(row.recipe_selection_rate, 1);
  assert.equal(row.oracle_isolation_rate, 1);
  assert.equal(row.clean_source_rate, 1);
  assert.equal(row.metadata_boundary_count, 1);
  assert.equal(row.metadata_mixed, false);
  assert.deepEqual(row.requested_models, ['gemma4:12b-mlx']);
  assert.deepEqual(row.declared_context_tokens, [16384]);
  assert.equal(row.gateway_context_observed, false);
  assert.equal(row.first_run_order, 4);
  assert.equal(row.last_run_order, 4);
  assert.equal(row.p50_elapsed_ms, 7000);
  assert.equal(row.p50_turn_elapsed_ms, 7000);
  assert.equal(row.p50_turn_burst_elapsed_ms, 6000);
  assert.equal(row.mean_turn_model_calls, 1);
  assert.equal(row.mean_turn_tool_calls, 1);
  assert.equal(row.mean_turn_deterministic_operations, 22);
  assert.equal(row.model_attempt_metric_samples, 2);
  assert.deepEqual(row.model_attempt_outcomes, { succeeded: 2 });
  assert.equal(row.model_attempt_success_rate, 1);
  assert.equal(row.prompt_token_metric_coverage, 1);
  assert.equal(row.completion_token_metric_coverage, 1);
  assert.equal(row.mean_request_body_bytes, 6400);
  assert.equal(row.mean_prompt_tokens, 800);
  assert.equal(row.mean_completion_tokens, 120);
  assert.equal(row.mean_request_duration_ms, 5900);
  assert.equal(row.gateway_model_duration_metric_coverage, 0);
  assert.equal(row.mean_gateway_model_duration_ms, null);
  assert.equal(row.mean_compiled_operations, 22);
  assert.equal(row.unique_route_semantic_identities, null);
  assert.equal(row.unique_adjudication_identities, null);
  assert.equal(row.unique_compiler_input_identities, null);
  assert.equal(row.unique_semantic_intent_identities, null);
  assert.equal(row.unique_compiled_plan_identities, null);
  assert.equal(row.unique_ruleset_identities, null);
  assert.deepEqual(row.canonical_ruleset_digests, []);
  assert.equal(row.semantic_ruleset_identity_consistent, false);
  assert.equal(row.semantic_ruleset_identity_eligible_reports, 2);
  assert.equal(row.semantic_ruleset_identity_covered_reports, 0);
  assert.equal(row.semantic_ruleset_identity_missing_reports, 2);
});

test('summary canonicalizes RuleSets and exposes constant semantic identity collisions', () => {
  const identity = (value) => value.repeat(64);
  const intent = (overrides = {}) => ({
    schema_version: 4,
    outcome: 'ready',
    completed: true,
    elapsed_ms: 1,
    final_intent: {
      status: 'preview_ready',
      route_decision: {
        semantic_ir_digest: overrides.routeSemantic || identity('1'),
        adjudication_digest: overrides.adjudication || identity('2'),
      },
      receipt: {
        compiler_input_hash: overrides.compilerInput || identity('3'),
        semantic_intent_hash: identity('4'),
        compiled_plan_hash: overrides.plan || identity('5'),
        candidate_ruleset_hash: overrides.candidateRuleset || identity('a'),
        compiled_operations: 22,
      },
    },
    ruleset: overrides.ruleset,
    observability: { model_calls: 1, tool_calls: 1, distinct_mutation_tools: [] },
    turns: [],
  });
  const canonical = {
    version: 1,
    panels: [{ key: 'study', buttons: [{ label: 'Create' }] }],
    modals: [],
    rules: [],
  };
  const reordered = {
    rules: [],
    modals: [],
    panels: [{ buttons: [{ label: 'Create' }], key: 'study' }],
    version: 1,
  };
  const changed = structuredClone(canonical);
  changed.panels[0].buttons[0].label = 'Start';
  const document = {
    results: {
      results: [
        intent({ ruleset: canonical }),
        intent({ ruleset: reordered }),
        intent({
          routeSemantic: identity('6'),
          adjudication: identity('7'),
          compilerInput: identity('8'),
          plan: identity('9'),
          candidateRuleset: identity('b'),
          ruleset: changed,
        }),
      ].map((report) => ({
        provider: { label: 'gemma-intent' },
        vars: { caseId: 'constant-semantic', cohort: 'intent_recipe' },
        success: true,
        response: { metadata: report },
      })),
    },
  };

  const [row] = summarize(document);

  assert.equal(row.unique_route_semantic_identities, 2);
  assert.equal(row.unique_adjudication_identities, 2);
  assert.equal(row.unique_compiler_input_identities, 2);
  assert.equal(row.unique_semantic_intent_identities, 1);
  assert.equal(row.unique_compiled_plan_identities, 2);
  assert.equal(row.unique_ruleset_identities, 2);
  assert.equal(row.canonical_ruleset_digests.length, 2);
  assert.equal(row.semantic_ruleset_identity_consistent, false);
});
