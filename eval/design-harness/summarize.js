const fs = require('node:fs');

function parseReport(row) {
  const metadata = row.response?.metadata;
  if (metadata && typeof metadata === 'object' && metadata.schema_version) {
    return metadata;
  }
  const output = row.response?.output ?? row.output;
  if (typeof output === 'string') {
    try {
      return JSON.parse(output);
    } catch {
      return null;
    }
  }
  return output && typeof output === 'object' ? output : null;
}

function rowsFrom(document) {
  if (Array.isArray(document.results?.results)) {
    return document.results.results;
  }
  return Array.isArray(document.results) ? document.results : [];
}

function finiteValues(values) {
  return values.filter((value) => Number.isFinite(value));
}

function mean(values) {
  const finite = finiteValues(values);
  return finite.length === 0 ? null : finite.reduce((sum, value) => sum + value, 0) / finite.length;
}

function percentile(values, fraction) {
  const sorted = finiteValues(values).sort((left, right) => left - right);
  if (sorted.length === 0) {
    return null;
  }
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)];
}

function providerName(row) {
  return row.provider?.label || row.provider?.id || (typeof row.provider === 'string' ? row.provider : 'unknown');
}

function vars(row) {
  return row.vars || row.testCase?.vars || {};
}

function postcheck(report, gate) {
  if (report?.schema_version === 3) {
    return report.actual_gates?.[`${gate === 'validate' ? 'validation' : 'simulation'}_current`] === true;
  }
  if (report?.schema_version === 2) {
    return report.postcheck?.[`${gate}_passed`] === true;
  }
  return report?.[`final_${gate}_passed`] === true;
}

function actualGate(report, gate) {
  if (report?.schema_version === 3) {
    return report.actual_gates?.[`${gate}_current`] === true;
  }
  if (report?.schema_version === 2) {
    return report.actual_gates?.[`${gate}_current`] === true;
  }
  return report?.[`${gate}_current`] === true;
}

function assertionPassed(row, name) {
  const components = row.gradingResult?.componentResults;
  if (!Array.isArray(components)) {
    return null;
  }
  const component = components.find((entry) => entry.assertion?.value?.endsWith(`:${name}`));
  return component ? component.pass === true : null;
}

function summarize(document) {
  const groups = new Map();
  for (const row of rowsFrom(document)) {
    const provider = providerName(row);
    const caseId = vars(row).caseId || row.description || 'unknown';
    const key = `${provider}::${caseId}`;
    const group = groups.get(key) || { provider, caseId, rows: [] };
    group.rows.push({ row, report: parseReport(row) });
    groups.set(key, group);
  }

  return [...groups.values()].map((group) => {
    const reports = group.rows.map((entry) => entry.report).filter(Boolean);
    const requiredSimulation = group.rows.some((entry) => vars(entry.row).requireSimulation === true);
    const elapsed = reports.map((report) => Number(report.elapsed_ms));
    const modelCalls = reports.map((report) => Number(report.observability?.model_calls));
    const toolCalls = reports.map((report) => Number(report.observability?.tool_calls));
    const distinctTools = reports.map((report) => Number(report.observability?.distinct_mutation_tools?.length));
    const repeatCounts = reports.map((report) => Number(report.max_repeat_count));
    const repairAttempts = reports.map((report) => Number(report.observability?.repair_attempts ?? 0));
    const repairSuccesses = reports.map((report) => Number(report.observability?.repair_successes ?? 0));
    const repairFailures = reports.map((report) => Number(report.observability?.repair_failures ?? 0));
    const repairEscalations = reports.map((report) => Number(report.observability?.repair_escalations ?? 0));
    const injectedControlCalls = reports.map((report) => Number(report.injected_control_calls ?? 0));
    const delegatedModelCalls = reports.map((report) => Number(report.delegated_model_calls));
    const planSubmissions = reports.map((report) => Number(report.observability?.plan_submissions ?? 0));
    const planAcceptances = reports.map((report) => Number(report.observability?.plan_acceptances ?? 0));
    const plannedRequirements = reports.map((report) => Number(report.observability?.planned_requirements ?? 0));
    const planCompiledToolCalls = reports.map((report) => Number(report.observability?.plan_compiled_tool_calls ?? 0));
    const planExecutionFailures = reports.map((report) => Number(report.observability?.plan_execution_failures ?? 0));
    const planRollbacks = reports.map((report) => Number(report.observability?.plan_rollbacks ?? 0));
    const planCommits = reports.map((report) => Number(report.observability?.plan_commits ?? 0));
    const planConflicts = reports.map((report) => Number(report.observability?.plan_conflicts ?? 0));
    const turns = reports.flatMap((report) => Array.isArray(report.turns) ? report.turns : []);
    const turnElapsed = turns.map((turn) => Number(turn.elapsed_ms));
    const turnModelCalls = turns.map((turn) => Number(turn.model_calls ?? turn.observability_delta?.model_calls));
    const turnToolCalls = turns.map((turn) => Number(turn.model_tool_calls ?? turn.observability_delta?.tool_calls));
    const turnDeterministicOperations = turns.map((turn) => Number(turn.deterministic_operations));
    const intentReports = reports.filter((report) => report.schema_version === 3);
    const metadataBoundaries = new Set(intentReports.map((report) => [
      report.requested_model,
      report.served_model,
      report.declared_context_tokens,
      report.gateway_id,
      report.provenance?.source_commit,
      report.provenance?.binary_sha256,
      report.final_intent?.binding_fingerprint,
      JSON.stringify(report.session_config),
      report.provenance?.run_id,
    ].join('::')));
    const requestedModels = [...new Set(intentReports.map((report) => report.requested_model))];
    const servedModels = [...new Set(intentReports.map((report) => report.served_model))];
    const gatewayIds = [...new Set(intentReports.map((report) => report.gateway_id))];
    const declaredContextTokens = [...new Set(intentReports.map((report) => report.declared_context_tokens))];
    const sourceCommits = [...new Set(intentReports.map((report) => report.provenance?.source_commit))];
    const runIds = [...new Set(intentReports.map((report) => report.provenance?.run_id))];
    const binaryDigests = [...new Set(intentReports.map((report) => report.provenance?.binary_sha256))];
    const bindingFingerprints = [...new Set(intentReports.map((report) => report.final_intent?.binding_fingerprint))];
    const runOrders = intentReports.map((report) => Number(report.provenance?.run_order));
    const started = intentReports.map((report) => Number(report.provenance?.started_at_unix_ms));
    const ended = intentReports.map((report) => Number(report.provenance?.ended_at_unix_ms));
    const receiptOperations = intentReports.map((report) => Number(report.final_intent?.receipt?.compiled_operations));
    const semanticRows = group.rows.filter((entry) => assertionPassed(entry.row, 'taskSemantics') !== null);
    return {
      provider: group.provider,
      case_id: group.caseId,
      cohort: intentReports.length > 0 ? 'intent_recipe' : 'legacy',
      runs: group.rows.length,
      valid_reports: reports.length,
      valid_report_rate: reports.length / group.rows.length,
      provider_error_rate: group.rows.filter((entry) => !entry.report || entry.row.response?.error).length / group.rows.length,
      pass_rate: group.rows.filter((entry) => entry.row.success === true).length / group.rows.length,
      exact_semantics_rate: semanticRows.length === 0
        ? null
        : semanticRows.filter((entry) => assertionPassed(entry.row, 'taskSemantics') === true).length / semanticRows.length,
      completion_rate: group.rows.filter((entry) => entry.report?.completed === true).length / group.rows.length,
      validation_rate: group.rows.filter((entry) => postcheck(entry.report, 'validate')).length / group.rows.length,
      required_simulation_rate: requiredSimulation
        ? group.rows.filter((entry) => postcheck(entry.report, 'simulate')).length / group.rows.length
        : null,
      actual_validation_current_rate: group.rows.filter((entry) => actualGate(entry.report, 'validation')).length / group.rows.length,
      actual_simulation_current_rate: group.rows.filter((entry) => actualGate(entry.report, 'simulation')).length / group.rows.length,
      ready_rate: group.rows.filter((entry) => ['ready', 'completed'].includes(entry.report?.outcome)).length / group.rows.length,
      clarification_rate: group.rows.filter((entry) => entry.report?.turns?.some((turn) => ['needs_input', 'awaiting_human'].includes(turn.outcome))).length / group.rows.length,
      repeated_error_rate: group.rows.filter((entry) => Number(entry.report?.max_repeat_count) > 1).length / group.rows.length,
      mean_max_repeat_count: mean(repeatCounts),
      maximum_repeat_count: finiteValues(repeatCounts).length === 0 ? null : Math.max(...finiteValues(repeatCounts)),
      mean_elapsed_ms: mean(elapsed) === null ? null : Math.round(mean(elapsed)),
      p50_elapsed_ms: percentile(elapsed, 0.5),
      p95_elapsed_ms: percentile(elapsed, 0.95),
      mean_model_calls: mean(modelCalls),
      mean_tool_calls: mean(toolCalls),
      mean_distinct_mutation_tools: mean(distinctTools),
      mean_repair_attempts: mean(repairAttempts),
      mean_repair_successes: mean(repairSuccesses),
      mean_repair_failures: mean(repairFailures),
      mean_repair_escalations: mean(repairEscalations),
      mean_injected_control_calls: mean(injectedControlCalls),
      mean_delegated_model_calls: mean(delegatedModelCalls),
      mean_plan_submissions: mean(planSubmissions),
      mean_plan_acceptances: mean(planAcceptances),
      mean_planned_requirements: mean(plannedRequirements),
      mean_plan_compiled_tool_calls: mean(planCompiledToolCalls),
      mean_plan_execution_failures: mean(planExecutionFailures),
      mean_plan_rollbacks: mean(planRollbacks),
      mean_plan_commits: mean(planCommits),
      mean_plan_conflicts: mean(planConflicts),
      mean_turns: mean(reports.map((report) => Number(report.turns?.length))),
      changed_turn_rate: turns.length === 0 ? null : turns.filter((turn) => turn.draft_changed === true).length / turns.length,
      needs_input_turn_rate: turns.length === 0 ? null : turns.filter((turn) => ['needs_input', 'awaiting_human'].includes(turn.outcome)).length / turns.length,
      mean_turn_elapsed_ms: mean(turnElapsed) === null ? null : Math.round(mean(turnElapsed)),
      p50_turn_elapsed_ms: percentile(turnElapsed, 0.5),
      p95_turn_elapsed_ms: percentile(turnElapsed, 0.95),
      mean_turn_model_calls: mean(turnModelCalls),
      mean_turn_tool_calls: mean(turnToolCalls),
      mean_turn_deterministic_operations: mean(turnDeterministicOperations),
      mean_compiled_operations: mean(receiptOperations),
      recipe_selection_rate: intentReports.length === 0
        ? null
        : intentReports.filter((report) => report.final_intent?.status === 'preview_ready').length / group.rows.length,
      oracle_isolation_rate: intentReports.length === 0
        ? null
        : intentReports.filter((report) => report.oracle?.enabled === false && report.oracle?.injected_control_calls === 0).length / group.rows.length,
      clean_source_rate: intentReports.length === 0
        ? null
        : intentReports.filter((report) => report.provenance?.source_dirty === false
          && report.provenance?.build_source_dirty === false).length / group.rows.length,
      metadata_boundary_count: intentReports.length === 0 ? null : metadataBoundaries.size,
      metadata_mixed: intentReports.length === 0 ? null : metadataBoundaries.size !== 1,
      requested_models: requestedModels,
      served_models: servedModels,
      gateway_ids: gatewayIds,
      declared_context_tokens: declaredContextTokens,
      gateway_context_observed: intentReports.some((report) => report.gateway_context_observed_tokens !== null),
      source_commits: sourceCommits,
      binary_sha256: binaryDigests,
      binding_fingerprints: bindingFingerprints,
      run_ids: runIds,
      first_run_order: finiteValues(runOrders).length === 0 ? null : Math.min(...finiteValues(runOrders)),
      last_run_order: finiteValues(runOrders).length === 0 ? null : Math.max(...finiteValues(runOrders)),
      started_at_unix_ms: finiteValues(started).length === 0 ? null : Math.min(...finiteValues(started)),
      ended_at_unix_ms: finiteValues(ended).length === 0 ? null : Math.max(...finiteValues(ended)),
    };
  });
}

if (require.main === module) {
  const input = process.argv[2];
  if (!input) {
    process.stderr.write('usage: node summarize.js <promptfoo-results.json>\n');
    process.exit(2);
  }
  const document = JSON.parse(fs.readFileSync(input, 'utf8'));
  process.stdout.write(`${JSON.stringify(summarize(document), null, 2)}\n`);
}

module.exports = { summarize };
