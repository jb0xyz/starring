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
    return {
      provider: group.provider,
      case_id: group.caseId,
      runs: group.rows.length,
      valid_reports: reports.length,
      valid_report_rate: reports.length / group.rows.length,
      provider_error_rate: group.rows.filter((entry) => !entry.report || entry.row.response?.error).length / group.rows.length,
      pass_rate: group.rows.filter((entry) => entry.row.success === true).length / group.rows.length,
      completion_rate: group.rows.filter((entry) => entry.report?.completed === true).length / group.rows.length,
      validation_rate: group.rows.filter((entry) => entry.report?.final_validate_passed === true).length / group.rows.length,
      required_simulation_rate: requiredSimulation
        ? group.rows.filter((entry) => entry.report?.final_simulate_passed === true).length / group.rows.length
        : null,
      repeated_error_rate: group.rows.filter((entry) => Number(entry.report?.max_repeat_count) > 1).length / group.rows.length,
      mean_max_repeat_count: mean(repeatCounts),
      maximum_repeat_count: finiteValues(repeatCounts).length === 0 ? null : Math.max(...finiteValues(repeatCounts)),
      mean_elapsed_ms: mean(elapsed) === null ? null : Math.round(mean(elapsed)),
      p95_elapsed_ms: percentile(elapsed, 0.95),
      mean_model_calls: mean(modelCalls),
      mean_tool_calls: mean(toolCalls),
      mean_distinct_mutation_tools: mean(distinctTools),
      mean_repair_attempts: mean(repairAttempts),
      mean_repair_successes: mean(repairSuccesses),
      mean_repair_failures: mean(repairFailures),
      mean_repair_escalations: mean(repairEscalations),
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
