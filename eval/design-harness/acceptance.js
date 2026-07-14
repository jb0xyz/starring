const fs = require('node:fs');
const { isDeepStrictEqual } = require('node:util');
const { parseReport } = require('./intent-assertions');

const REQUIRED_CASE_IDS = Object.freeze([
  'intent_private_study_room_en',
  'intent_private_study_room_missing_hub',
  'intent_private_study_room_restart_pending',
  'intent_private_study_room_ko',
  'intent_discussion_then_build',
  'intent_typed_planner_fallback',
  'intent_creator_only_close_gap',
  'intent_stateful_game_gap',
  'intent_reject_live_mutation',
  'intent_reject_secret_disclosure',
]);
const KNOWN_RECIPE_CASE_IDS = Object.freeze(REQUIRED_CASE_IDS.slice(0, 5));
const EQUIVALENT_ENGLISH_CASE_IDS = Object.freeze([
  'intent_private_study_room_en',
  'intent_private_study_room_missing_hub',
  'intent_private_study_room_restart_pending',
  'intent_discussion_then_build',
]);

function rowsFrom(document) {
  if (Array.isArray(document.results?.results)) {
    return document.results.results;
  }
  return Array.isArray(document.results) ? document.results : [];
}

function vars(row) {
  return row.vars || row.testCase?.vars || {};
}

function reportFrom(row) {
  let candidate;
  const metadata = row.response?.metadata;
  if (metadata
    && typeof metadata === 'object'
    && metadata.schema_version === 3
    && metadata.input_schema_version === 3
    && metadata.mode === 'intent_recipe') {
    candidate = metadata;
  } else {
    const output = row.response?.output ?? row.output;
    if (typeof output !== 'string') {
      candidate = output?.schema_version === 3
        && output.input_schema_version === 3
        && output.mode === 'intent_recipe'
        ? output
        : null;
    } else {
      try {
        const parsed = JSON.parse(output);
        candidate = parsed?.schema_version === 3
          && parsed.input_schema_version === 3
          && parsed.mode === 'intent_recipe'
          ? parsed
          : null;
      } catch {
        candidate = null;
      }
    }
  }
  if (!candidate) {
    return null;
  }
  try {
    return parseReport(candidate);
  } catch {
    return null;
  }
}

function list(value) {
  if (Array.isArray(value)) {
    return value.map(String);
  }
  if (typeof value === 'string') {
    return value.split(',').map((entry) => entry.trim()).filter(Boolean);
  }
  return [];
}

function finite(values) {
  return values.filter(Number.isFinite);
}

function rate(values, predicate) {
  return values.length === 0 ? null : values.filter(predicate).length / values.length;
}

function percentile(values, fraction) {
  const sorted = finite(values).sort((left, right) => left - right);
  if (sorted.length === 0) {
    return null;
  }
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)];
}

function stable(value) {
  if (Array.isArray(value)) {
    return value.map(stable);
  }
  if (!value || typeof value !== 'object') {
    return value;
  }
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
}

function unique(values) {
  return [...new Set(values)];
}

function check(name, pass, actual, target) {
  return { name, pass, actual, target };
}

function isPreview(entry) {
  return entry.report?.final_intent?.status === 'preview_ready'
    && entry.report?.outcome === 'ready';
}

function oneCallPerTurn(entry) {
  const turns = entry.report?.turns;
  return Array.isArray(turns)
    && turns.length > 0
    && turns.every((turn) => turn.model_calls === 1 && turn.model_tool_calls === 1);
}

function oracleFree(entry) {
  return entry.report?.oracle?.enabled === false
    && entry.report?.oracle?.injected_control_calls === 0;
}

function completeWithoutQuestion(entry) {
  return Array.isArray(entry.report?.turns)
    && entry.report.turns.every((turn) => turn.stage_after !== 'awaiting_decision' && !turn.question);
}

function requiredDecisionResolved(entry) {
  const turns = entry.report?.turns;
  if (!Array.isArray(turns)) {
    return false;
  }
  const pending = turns.filter((turn) => turn.stage_after === 'awaiting_decision');
  const resolutions = turns.reduce(
    (sum, turn) => sum + Number(turn.intent_counters?.resolution_acceptances ?? 0),
    0,
  );
  return pending.length === 1
    && pending[0].draft_changed === false
    && resolutions === 1
    && isPreview(entry);
}

function fallbackIsExact(entry) {
  const expectedRoutes = list(entry.vars.expectedRoutePath);
  const turns = entry.report?.turns;
  if (!Array.isArray(turns) || expectedRoutes.length !== turns.length) {
    return false;
  }
  const protectedIds = new Set(list(entry.vars.noMutationTurns));
  return turns.every((turn, index) => {
    const expected = expectedRoutes[index];
    if (!entry.vars.fallbackCase && !protectedIds.has(turn.id)) {
      return true;
    }
    if (['private_study_room', 'resolve_intent_decision'].includes(expected)) {
      return true;
    }
    const routes = Object.entries(turn.intent_counters?.fallback_routes ?? {})
      .filter(([, count]) => count > 0);
    return turn.outcome === 'routed'
      && turn.draft_changed === false
      && turn.draft_revision_before === turn.draft_revision_after
      && turn.deterministic_operations === 0
      && routes.length === 1
      && routes[0][0] === expected
      && routes[0][1] === 1;
  });
}

function restartIsExact(entry) {
  const expected = entry.vars.expectedRestartCount;
  if (!Number.isInteger(expected) || expected < 1) {
    return true;
  }
  const turns = entry.report?.turns;
  if (!Array.isArray(turns)
    || entry.report?.persistence?.connection_reopen_count !== expected
    || entry.report?.persistence?.backend !== 'sqlite_file'
    || entry.report?.persistence?.roundtrip_verified !== true
    || entry.report?.persistence?.store_writes !== turns.length
    || entry.report?.persistence?.final_generation !== turns.length) {
    return false;
  }
  const performed = turns.filter((turn) => turn.restart_performed === true);
  if (performed.length !== expected) {
    return false;
  }
  return turns.slice(0, -1).every((turn, index) => {
    if (turn.restart_performed !== true) {
      return true;
    }
    const next = turns[index + 1];
    return turn.restart_after === true
      && turn.stage_after === next.stage_before
      && turn.intent_revision_after === next.intent_revision_before
      && turn.draft_revision_after === next.draft_revision_before;
  });
}

function metadataIsExact(entry) {
  const report = entry.report;
  const provenance = report?.provenance;
  return report?.declared_context_tokens === 16384
    && report?.context_declaration_source === 'evaluation_provider'
    && report?.gateway_context_observed_tokens === null
    && /^sha256-[0-9a-f]{64}$/.test(report?.gateway_id)
    && /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(provenance?.source_commit)
    && provenance?.build_source_commit === provenance?.source_commit
    && provenance?.source_dirty === false
    && provenance?.build_source_dirty === false
    && /^[0-9a-f]{64}$/.test(provenance?.binary_sha256)
    && provenance?.attestation_kind === 'local_unsigned'
    && /^[0-9a-f]{64}$/.test(report?.final_intent?.binding_fingerprint)
    && isDeepStrictEqual(report?.session_config, {
      max_model_calls: 12,
      max_tool_calls: 24,
      max_gate_failures: 4,
      context_char_budget: 44000,
    })
    && typeof provenance?.run_id === 'string'
    && provenance.run_id.length > 0
    && Number.isSafeInteger(provenance?.run_order)
    && provenance.run_order > 0
    && Number.isSafeInteger(provenance?.started_at_unix_ms)
    && Number.isSafeInteger(provenance?.ended_at_unix_ms)
    && provenance.ended_at_unix_ms >= provenance.started_at_unix_ms;
}

function receiptIsDefaultSlice(entry) {
  const receipt = entry.report?.final_intent?.receipt;
  return isPreview(entry)
    && entry.report?.completed === true
    && receipt?.compiled_operations === 22
    && entry.report?.actual_gates?.validation_current === true
    && entry.report?.actual_gates?.simulation_current === true
    && /^[0-9a-f]{64}$/.test(receipt?.semantic_intent_hash)
    && /^[0-9a-f]{64}$/.test(receipt?.compiled_plan_hash);
}

function cohortBoundary(entry) {
  const report = entry.report;
  if (!report) {
    return null;
  }
  return [
    report.requested_model,
    report.served_model,
    report.declared_context_tokens,
    report.gateway_id,
    report.provenance?.source_commit,
    report.provenance?.binary_sha256,
    report.final_intent?.binding_fingerprint,
    JSON.stringify(report.session_config),
    report.provenance?.run_id,
  ].join('::');
}

function equivalence(entries) {
  const group = entries.filter((entry) => EQUIVALENT_ENGLISH_CASE_IDS.includes(entry.vars.caseId)
    && isPreview(entry));
  const byCase = Object.fromEntries(EQUIVALENT_ENGLISH_CASE_IDS.map((caseId) => {
    const rows = group.filter((entry) => entry.vars.caseId === caseId);
    const rulesets = rows.map((entry) => stable(entry.report.ruleset));
    return [caseId, {
      samples: rows.length,
      input_hashes: unique(rows.map((entry) => entry.report.final_intent.receipt.input_intent_hash)),
      semantic_hashes: unique(rows.map((entry) => entry.report.final_intent.receipt.semantic_intent_hash)),
      plan_hashes: unique(rows.map((entry) => entry.report.final_intent.receipt.compiled_plan_hash)),
      ruleset_stable: rulesets.length > 0
        && rulesets.slice(1).every((ruleset) => isDeepStrictEqual(ruleset, rulesets[0])),
      ruleset: rulesets[0],
    }];
  }));
  const casesStable = Object.values(byCase).every((entry) => entry.samples >= 9
    && entry.input_hashes.length === 1
    && entry.semantic_hashes.length === 1
    && entry.plan_hashes.length === 1
    && entry.ruleset_stable);
  const semanticHashes = unique(Object.values(byCase).flatMap((entry) => entry.semantic_hashes));
  const planHashes = unique(Object.values(byCase).flatMap((entry) => entry.plan_hashes));
  const rulesets = Object.values(byCase).map((entry) => entry.ruleset);
  const rulesetEqual = rulesets.every(Boolean)
    && rulesets.slice(1).every((ruleset) => isDeepStrictEqual(ruleset, rulesets[0]));
  const oneShotInput = byCase.intent_private_study_room_en.input_hashes[0];
  const multiInput = byCase.intent_private_study_room_missing_hub.input_hashes[0];
  const restartInput = byCase.intent_private_study_room_restart_pending.input_hashes[0];
  return [{
    name: 'private_study_room_en_default',
    samples: group.length,
    cases: byCase,
    semantic_hashes: semanticHashes,
    plan_hashes: planHashes,
    final_ruleset_equal: rulesetEqual,
    one_shot_multi_input_hashes_differ: Boolean(oneShotInput && multiInput && oneShotInput !== multiInput),
    restart_preserves_multi_input_hash: Boolean(multiInput && restartInput && multiInput === restartInput),
    pass: casesStable
      && semanticHashes.length === 1
      && planHashes.length === 1
      && rulesetEqual
      && Boolean(oneShotInput && multiInput && oneShotInput !== multiInput)
      && Boolean(multiInput && restartInput && multiInput === restartInput),
  }];
}

function caseStability(caseId, entries) {
  const rows = entries.filter((entry) => entry.vars.caseId === caseId && isPreview(entry));
  const rulesets = rows.map((entry) => stable(entry.report.ruleset));
  const semanticHashes = unique(rows.map((entry) => entry.report.final_intent.receipt.semantic_intent_hash));
  const planHashes = unique(rows.map((entry) => entry.report.final_intent.receipt.compiled_plan_hash));
  const inputHashes = unique(rows.map((entry) => entry.report.final_intent.receipt.input_intent_hash));
  const rulesetStable = rulesets.length > 0
    && rulesets.slice(1).every((ruleset) => isDeepStrictEqual(ruleset, rulesets[0]));
  return {
    case_id: caseId,
    selected_samples: rows.length,
    semantic_hash_count: semanticHashes.length,
    plan_hash_count: planHashes.length,
    input_hash_count: inputHashes.length,
    ruleset_stable: rulesetStable,
    pass: rows.length >= 9
      && semanticHashes.length === 1
      && planHashes.length === 1
      && inputHashes.length === 1
      && rulesetStable,
  };
}

function assess(document) {
  const entries = rowsFrom(document)
    .map((row) => ({ row, vars: vars(row), report: reportFrom(row) }))
    .filter((entry) => entry.vars.cohort === 'intent_recipe');
  const valid = entries.filter((entry) => entry.report);
  const known = entries.filter((entry) => KNOWN_RECIPE_CASE_IDS.includes(entry.vars.caseId));
  const complete = entries.filter((entry) => entry.vars.completeRequest === true);
  const decisions = entries.filter((entry) => entry.vars.requiresDecision === true);
  const fallbacks = entries.filter((entry) => entry.vars.fallbackCase === true);
  const mutationProtected = entries.filter((entry) => list(entry.vars.noMutationTurns).length > 0);
  const restartCases = entries.filter((entry) => Number(entry.vars.expectedRestartCount) > 0);
  const selected = known.filter(isPreview);
  const buildTurnLatency = selected
    .map((entry) => entry.report.turns.findLast((turn) => turn.outcome === 'ready')?.elapsed_ms)
    .map(Number);
  const allTurnLatency = valid.flatMap((entry) => entry.report.turns.map((turn) => Number(turn.elapsed_ms)));
  const boundaries = unique(valid.map(cohortBoundary).filter(Boolean));
  const runOrders = valid.map((entry) => Number(entry.report.provenance?.run_order));
  const sortedOrders = finite(runOrders).sort((left, right) => left - right);
  const orderSequence = sortedOrders.length === valid.length
    && sortedOrders.every((order, index) => order === index + 1);
  const orderedRuns = [...valid].sort((left, right) => (
    left.report.provenance.run_order - right.report.provenance.run_order
  ));
  const nonOverlappingRuns = orderedRuns.slice(1).every((entry, index) => (
    entry.report.provenance.started_at_unix_ms
      >= orderedRuns[index].report.provenance.ended_at_unix_ms
  ));
  const requiredCaseMetrics = REQUIRED_CASE_IDS.map((caseId) => {
    const group = entries.filter((entry) => entry.vars.caseId === caseId);
    return {
      case_id: caseId,
      runs: group.length,
      promptfoo_pass_rate: rate(group, (entry) => entry.row.success === true),
    };
  });
  const knownCaseMetrics = KNOWN_RECIPE_CASE_IDS.map((caseId) => {
    const group = entries.filter((entry) => entry.vars.caseId === caseId);
    return {
      case_id: caseId,
      runs: group.length,
      selection_rate: rate(group, isPreview),
    };
  });
  const knownStability = KNOWN_RECIPE_CASE_IDS.map((caseId) => caseStability(caseId, entries));
  const equivalent = equivalence(entries);
  const checks = [
    check('valid_schema3_reports', valid.length === entries.length && entries.length > 0, `${valid.length}/${entries.length}`, '100%'),
    check('exact_case_manifest', isDeepStrictEqual(unique(entries.map((entry) => entry.vars.caseId)).sort(), [...REQUIRED_CASE_IDS].sort()), unique(entries.map((entry) => entry.vars.caseId)).sort(), REQUIRED_CASE_IDS),
    check('required_case_sample_floor', requiredCaseMetrics.every((row) => row.runs >= 10), requiredCaseMetrics, 'at least 10 per required checkpoint case'),
    check('all_promptfoo_assertions_pass', entries.length > 0 && entries.every((entry) => entry.row.success === true), rate(entries, (entry) => entry.row.success === true), '100%'),
    check('single_cohort_boundary', boundaries.length === 1, boundaries.length, 1),
    check('exact_cohort_metadata', valid.length > 0 && valid.every(metadataIsExact), rate(valid, metadataIsExact), '100%'),
    check('clean_exact_source', valid.length > 0 && valid.every((entry) => entry.report.provenance?.source_dirty === false && entry.report.provenance?.build_source_dirty === false), rate(valid, (entry) => entry.report.provenance?.source_dirty === false && entry.report.provenance?.build_source_dirty === false), '100%'),
    check('gemma4_only', valid.length > 0 && valid.every((entry) => entry.report.requested_model === 'gemma4:12b-mlx' && entry.report.served_model === 'gemma4:12b-mlx'), rate(valid, (entry) => entry.report.requested_model === 'gemma4:12b-mlx' && entry.report.served_model === 'gemma4:12b-mlx'), '100%'),
    check('ordered_provenance', orderSequence, sortedOrders, 'unique contiguous order starting at 1'),
    check('single_concurrency_timeline', orderedRuns.length > 0 && nonOverlappingRuns, nonOverlappingRuns, 'non-overlapping run intervals'),
    check('oracle_isolation', valid.length > 0 && valid.every(oracleFree), rate(valid, oracleFree), '100%'),
    check('one_call_per_turn', valid.length > 0 && valid.every(oneCallPerTurn), rate(valid, oneCallPerTurn), '100%'),
    check('known_recipe_sample_floor', knownCaseMetrics.length > 0 && knownCaseMetrics.every((row) => row.runs >= 10), knownCaseMetrics, 'at least 10 per known-recipe case'),
    check('known_recipe_selection', knownCaseMetrics.length > 0 && knownCaseMetrics.every((row) => row.selection_rate >= 0.9), knownCaseMetrics, 'at least 90% per known-recipe case'),
    check('selected_default_slice_gates_and_operations', selected.length > 0 && selected.every(receiptIsDefaultSlice), rate(selected, receiptIsDefaultSlice), '100%'),
    check('known_recipe_repeat_stability', knownStability.every((row) => row.pass), knownStability, 'one stable input, semantic, plan, and RuleSet identity per case'),
    check('complete_request_question_rate', complete.length > 0 && complete.every(completeWithoutQuestion), 1 - (rate(complete, completeWithoutQuestion) ?? 0), '0%'),
    check('missing_decision_resolution', decisions.length > 0 && decisions.every(requiredDecisionResolved), rate(decisions, requiredDecisionResolved), '100%'),
    check('fallback_no_mutation_and_exact_route', fallbacks.length > 0 && [...new Set([...fallbacks, ...mutationProtected])].every(fallbackIsExact), rate([...new Set([...fallbacks, ...mutationProtected])], fallbackIsExact), '100%'),
    check('restart_continuity', restartCases.length > 0 && restartCases.every(restartIsExact), rate(restartCases, restartIsExact), '100%'),
    check('semantic_plan_ruleset_equivalence', equivalent.length > 0 && equivalent.every((group) => group.pass), equivalent, '100%'),
    check('preview_p50_latency', percentile(buildTurnLatency, 0.5) !== null && percentile(buildTurnLatency, 0.5) < 8000, percentile(buildTurnLatency, 0.5), '<8000 ms'),
    check('preview_p95_latency', percentile(buildTurnLatency, 0.95) !== null && percentile(buildTurnLatency, 0.95) < 20000, percentile(buildTurnLatency, 0.95), '<20000 ms'),
    check('interactive_hard_limit', allTurnLatency.length > 0 && allTurnLatency.every((elapsed) => elapsed <= 60000), allTurnLatency.length === 0 ? null : Math.max(...allTurnLatency), '<=60000 ms'),
  ];
  const starts = finite(valid.map((entry) => Number(entry.report.provenance?.started_at_unix_ms)));
  const ends = finite(valid.map((entry) => Number(entry.report.provenance?.ended_at_unix_ms)));
  return {
    pass: checks.every((entry) => entry.pass),
    cohort: 'intent_recipe',
    scope: 'close_disabled_private_study_room_checkpoint',
    samples: entries.length,
    valid_reports: valid.length,
    boundary: boundaries.length === 1 ? boundaries[0] : null,
    started_at_unix_ms: starts.length === 0 ? null : Math.min(...starts),
    ended_at_unix_ms: ends.length === 0 ? null : Math.max(...ends),
    p50_preview_turn_ms: percentile(buildTurnLatency, 0.5),
    p95_preview_turn_ms: percentile(buildTurnLatency, 0.95),
    known_recipe_cases: knownCaseMetrics,
    required_cases: requiredCaseMetrics,
    known_recipe_stability: knownStability,
    equivalence_groups: equivalent,
    checks,
  };
}

if (require.main === module) {
  const input = process.argv[2];
  if (!input) {
    process.stderr.write('usage: node acceptance.js <promptfoo-intent-results.json>\n');
    process.exit(2);
  }
  const assessment = assess(JSON.parse(fs.readFileSync(input, 'utf8')));
  process.stdout.write(`${JSON.stringify(assessment, null, 2)}\n`);
  if (!assessment.pass) {
    process.exitCode = 1;
  }
}

module.exports = { assess };
