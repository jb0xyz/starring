const fs = require('node:fs');
const { createHash } = require('node:crypto');
const { isDeepStrictEqual } = require('node:util');
const { parseReport } = require('./intent-assertions');

const REQUIRED_CASE_IDS = Object.freeze([
  'intent_private_study_room_en',
  'intent_private_study_room_mutation_hub',
  'intent_private_study_room_mutation_naming',
  'intent_private_study_room_mutation_control',
  'intent_private_study_room_mutation_close',
  'intent_private_study_room_missing_hub',
  'intent_private_study_room_restart_pending',
  'intent_private_study_room_ko',
  'intent_discussion_then_build',
  'intent_typed_planner_fallback',
  'intent_creator_only_close_gap',
  'intent_stateful_game_gap',
  'intent_reject_live_mutation',
  'intent_reject_secret_disclosure',
  'intent_reject_skip_approval',
  'intent_reject_all_gate_bypass',
  'intent_redaction_copy_typed_planner',
  'intent_unknown_external_capability_gap',
  'intent_private_study_room_custom_details',
  'intent_private_study_room_custom_copy_only',
]);
const KNOWN_RECIPE_CASE_IDS = Object.freeze([
  'intent_private_study_room_en',
  'intent_private_study_room_missing_hub',
  'intent_private_study_room_restart_pending',
  'intent_private_study_room_ko',
  'intent_discussion_then_build',
  'intent_private_study_room_custom_details',
  'intent_private_study_room_custom_copy_only',
]);
const EQUIVALENT_ENGLISH_CASE_IDS = Object.freeze([
  'intent_private_study_room_en',
  'intent_private_study_room_missing_hub',
  'intent_private_study_room_restart_pending',
  'intent_discussion_then_build',
]);
const MUTATION_GROUPS = Object.freeze({
  hub: 'intent_private_study_room_mutation_hub',
  locale: 'intent_private_study_room_ko',
  close: 'intent_private_study_room_mutation_close',
  copy: 'intent_private_study_room_custom_copy_only',
  naming: 'intent_private_study_room_mutation_naming',
  control: 'intent_private_study_room_mutation_control',
});

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
    && metadata.schema_version === 5
    && metadata.input_schema_version === 3
    && metadata.mode === 'intent_recipe') {
    candidate = metadata;
  } else {
    const output = row.response?.output ?? row.output;
    if (typeof output !== 'string') {
      candidate = output?.schema_version === 5
        && output.input_schema_version === 3
        && output.mode === 'intent_recipe'
        ? output
        : null;
    } else {
      try {
        const parsed = JSON.parse(output);
        candidate = parsed?.schema_version === 5
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

function canonicalDigest(value) {
  return createHash('sha256').update(JSON.stringify(stable(value))).digest('hex');
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

function exactCallsPerTurn(entry) {
  const turns = entry.report?.turns;
  if (!Array.isArray(turns) || turns.length === 0) {
    return false;
  }
  const expectedModelCalls = Object.hasOwn(entry.vars, 'expectedModelCallsPerTurn')
    ? list(entry.vars.expectedModelCallsPerTurn).map(Number)
    : turns.map(() => 1);
  const expectedToolCalls = Object.hasOwn(entry.vars, 'expectedToolCallsPerTurn')
    ? list(entry.vars.expectedToolCallsPerTurn).map(Number)
    : turns.map(() => 1);
  if (expectedModelCalls.length !== turns.length || expectedToolCalls.length !== turns.length) {
    return false;
  }
  if (expectedModelCalls.some((value) => ![1, 2].includes(value))
    || expectedToolCalls.some((value) => ![1, 2].includes(value))) {
    return false;
  }
  const exactTurns = turns.every((turn, index) => (
    turn.model_calls === expectedModelCalls[index]
      && turn.model_tool_calls === expectedToolCalls[index]
  ));
  return exactTurns
    && entry.report.observability?.model_calls === expectedModelCalls.reduce((sum, value) => sum + value, 0)
    && entry.report.observability?.tool_calls === expectedToolCalls.reduce((sum, value) => sum + value, 0);
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
    && report?.catalog_identity?.recipe_id === 'starring.private_study_room'
    && report?.catalog_identity?.recipe_version === 1
    && report?.catalog_identity?.extractor_revision === 6
    && report?.catalog_identity?.normalizer_revision === 2
    && report?.catalog_identity?.compiler_revision === 1
    && report?.catalog_identity?.simulator_revision === 1
    && /^[0-9a-f]{64}$/.test(report?.catalog_identity?.registry_digest)
    && report?.model_call_metrics?.every((metric) => (
      Number.isSafeInteger(metric.prompt_tokens)
        && Number.isSafeInteger(metric.completion_tokens)
    ))
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
    JSON.stringify(stable(report.catalog_identity)),
    report.final_intent?.binding_fingerprint,
    JSON.stringify(stable(report.session_config)),
    report.provenance?.run_id,
  ].join('::');
}

function equivalence(entries) {
  const group = entries.filter((entry) => EQUIVALENT_ENGLISH_CASE_IDS.includes(entry.vars.caseId)
    && isPreview(entry));
  const byCase = Object.fromEntries(EQUIVALENT_ENGLISH_CASE_IDS.map((caseId) => {
    const rows = group.filter((entry) => entry.vars.caseId === caseId);
    const rulesets = rows.map((entry) => stable(entry.report.ruleset));
    const rulesetDigests = unique(rulesets.map(canonicalDigest));
    return [caseId, {
      samples: rows.length,
      compiler_input_hashes: unique(rows.map((entry) => entry.report.final_intent.receipt.compiler_input_hash)),
      semantic_hashes: unique(rows.map((entry) => entry.report.final_intent.receipt.semantic_intent_hash)),
      plan_hashes: unique(rows.map((entry) => entry.report.final_intent.receipt.compiled_plan_hash)),
      ruleset_digests: rulesetDigests,
      ruleset_stable: rulesets.length > 0
        && rulesetDigests.length === 1
        && rulesets.slice(1).every((ruleset) => isDeepStrictEqual(ruleset, rulesets[0])),
      ruleset: rulesets[0],
    }];
  }));
  const casesStable = Object.values(byCase).every((entry) => entry.samples >= 9
    && entry.compiler_input_hashes.length === 1
    && entry.semantic_hashes.length === 1
    && entry.plan_hashes.length === 1
    && entry.ruleset_stable);
  const semanticHashes = unique(Object.values(byCase).flatMap((entry) => entry.semantic_hashes));
  const planHashes = unique(Object.values(byCase).flatMap((entry) => entry.plan_hashes));
  const rulesets = Object.values(byCase).map((entry) => entry.ruleset);
  const rulesetEqual = rulesets.every(Boolean)
    && rulesets.slice(1).every((ruleset) => isDeepStrictEqual(ruleset, rulesets[0]));
  const oneShotInput = byCase.intent_private_study_room_en.compiler_input_hashes[0];
  const multiInput = byCase.intent_private_study_room_missing_hub.compiler_input_hashes[0];
  const restartInput = byCase.intent_private_study_room_restart_pending.compiler_input_hashes[0];
  return [{
    name: 'private_study_room_en_default',
    samples: group.length,
    cases: byCase,
    semantic_hashes: semanticHashes,
    plan_hashes: planHashes,
    final_ruleset_equal: rulesetEqual,
    one_shot_multi_compiler_input_hashes_differ: Boolean(oneShotInput && multiInput && oneShotInput !== multiInput),
    restart_preserves_multi_compiler_input_hash: Boolean(multiInput && restartInput && multiInput === restartInput),
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
  const rulesetDigests = unique(rulesets.map(canonicalDigest));
  const semanticHashes = unique(rows.map((entry) => entry.report.final_intent.receipt.semantic_intent_hash));
  const planHashes = unique(rows.map((entry) => entry.report.final_intent.receipt.compiled_plan_hash));
  const compilerInputHashes = unique(rows.map((entry) => entry.report.final_intent.receipt.compiler_input_hash));
  const rulesetStable = rulesets.length > 0
    && rulesets.slice(1).every((ruleset) => isDeepStrictEqual(ruleset, rulesets[0]));
  return {
    case_id: caseId,
    selected_samples: rows.length,
    semantic_hash_count: semanticHashes.length,
    plan_hash_count: planHashes.length,
    compiler_input_hash_count: compilerInputHashes.length,
    ruleset_digest_count: rulesetDigests.length,
    ruleset_stable: rulesetStable,
    pass: rows.length >= 9
      && semanticHashes.length === 1
      && planHashes.length === 1
      && compilerInputHashes.length === 1
      && rulesetDigests.length === 1
      && rulesetStable,
  };
}

function routeAdjudicationStability(caseId, entries) {
  const rows = entries.filter((entry) => entry.vars.caseId === caseId && entry.report);
  const decisions = rows.map((entry) => entry.report.final_intent.route_decision);
  const routeSemanticHashes = unique(
    decisions.filter(Boolean).map((decision) => decision.semantic_ir_digest),
  );
  const adjudicationHashes = unique(
    decisions.filter(Boolean).map((decision) => decision.adjudication_digest),
  );
  return {
    case_id: caseId,
    samples: rows.length,
    decision_samples: decisions.filter(Boolean).length,
    route_semantic_identity_count: routeSemanticHashes.length,
    adjudication_identity_count: adjudicationHashes.length,
    route_semantic_hashes: routeSemanticHashes,
    adjudication_hashes: adjudicationHashes,
    pass: rows.length > 0
      && decisions.every(Boolean)
      && routeSemanticHashes.length === 1
      && adjudicationHashes.length === 1,
  };
}

function semanticRulesetIdentity(entries) {
  const pairs = entries.filter(isPreview).map((entry) => ({
    case_id: entry.vars.caseId,
    semantic: entry.report.final_intent.receipt.semantic_intent_hash,
    ruleset: entry.report.final_intent.receipt.candidate_ruleset_hash,
  }));
  const semanticToRulesets = new Map();
  const rulesetToSemantics = new Map();
  for (const pair of pairs) {
    const rulesets = semanticToRulesets.get(pair.semantic) || new Set();
    rulesets.add(pair.ruleset);
    semanticToRulesets.set(pair.semantic, rulesets);
    const semantics = rulesetToSemantics.get(pair.ruleset) || new Set();
    semantics.add(pair.semantic);
    rulesetToSemantics.set(pair.ruleset, semantics);
  }
  const semanticCollisions = [...semanticToRulesets.entries()]
    .filter(([, identities]) => identities.size !== 1)
    .map(([identity, identities]) => ({ semantic_intent_hash: identity, ruleset_digests: [...identities].sort() }));
  const rulesetAliases = [...rulesetToSemantics.entries()]
    .filter(([, identities]) => identities.size !== 1)
    .map(([identity, identities]) => ({ ruleset_digest: identity, semantic_intent_hashes: [...identities].sort() }));
  return {
    samples: pairs.length,
    semantic_identity_count: semanticToRulesets.size,
    ruleset_identity_count: rulesetToSemantics.size,
    semantic_collisions: semanticCollisions,
    ruleset_aliases: rulesetAliases,
    pass: pairs.length > 0 && semanticCollisions.length === 0,
  };
}

function mutationMatrix(entries) {
  const baselineRows = entries.filter((entry) => (
    entry.vars.caseId === 'intent_private_study_room_en' && isPreview(entry)
  ));
  const baseline = {
    request: unique(baselineRows.map((entry) => entry.report.final_intent.receipt.request_evidence_hash)),
    route: unique(baselineRows.map((entry) => entry.report.final_intent.route_decision?.semantic_ir_digest)),
    adjudication: unique(baselineRows.map((entry) => entry.report.final_intent.route_decision?.adjudication_digest)),
    compiler: unique(baselineRows.map((entry) => entry.report.final_intent.receipt.compiler_input_hash)),
    semantic: unique(baselineRows.map((entry) => entry.report.final_intent.receipt.semantic_intent_hash)),
    plan: unique(baselineRows.map((entry) => entry.report.final_intent.receipt.compiled_plan_hash)),
    ruleset: unique(baselineRows.map((entry) => entry.report.final_intent.receipt.candidate_ruleset_hash)),
    draft: unique(baselineRows.map((entry) => entry.report.final_intent.receipt.candidate_draft_hash)),
  };
  const baselineStable = Object.values(baseline).every((identities) => identities.length === 1);
  return Object.entries(MUTATION_GROUPS).map(([group, caseId]) => {
    const rows = entries.filter((entry) => entry.vars.caseId === caseId && isPreview(entry));
    const identities = {
      request: unique(rows.map((entry) => entry.report.final_intent.receipt.request_evidence_hash)),
      route: unique(rows.map((entry) => entry.report.final_intent.route_decision?.semantic_ir_digest)),
      adjudication: unique(rows.map((entry) => entry.report.final_intent.route_decision?.adjudication_digest)),
      compiler: unique(rows.map((entry) => entry.report.final_intent.receipt.compiler_input_hash)),
      semantic: unique(rows.map((entry) => entry.report.final_intent.receipt.semantic_intent_hash)),
      plan: unique(rows.map((entry) => entry.report.final_intent.receipt.compiled_plan_hash)),
      ruleset: unique(rows.map((entry) => entry.report.final_intent.receipt.candidate_ruleset_hash)),
      draft: unique(rows.map((entry) => entry.report.final_intent.receipt.candidate_draft_hash)),
    };
    const stable = Object.values(identities).every((values) => values.length === 1);
    const distinct = Object.keys(identities).every((identity) => (
      identities[identity][0] !== baseline[identity][0]
    ));
    return {
      group,
      case_id: caseId,
      samples: rows.length,
      identities,
      stable,
      distinct_from_default: distinct,
      pass: baselineStable && rows.length >= 3 && stable && distinct,
    };
  });
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
  const readyTurns = selected
    .map((entry) => entry.report.turns.findLast((turn) => turn.outcome === 'ready'))
    .filter(Boolean);
  const oneCallBuildTurnLatency = readyTurns
    .filter((turn) => turn.model_calls === 1)
    .map((turn) => Number(turn.elapsed_ms));
  const twoCallBuildTurnLatency = readyTurns
    .filter((turn) => turn.model_calls === 2)
    .map((turn) => Number(turn.elapsed_ms));
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
    const minimumRuns = Object.values(MUTATION_GROUPS).includes(caseId)
      && !KNOWN_RECIPE_CASE_IDS.includes(caseId)
      ? 3
      : 10;
    return {
      case_id: caseId,
      runs: group.length,
      minimum_runs: minimumRuns,
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
  const routeAdjudication = REQUIRED_CASE_IDS.map(
    (caseId) => routeAdjudicationStability(caseId, entries),
  );
  const equivalent = equivalence(entries);
  const semanticRuleset = semanticRulesetIdentity(entries);
  const mutations = mutationMatrix(entries);
  const checks = [
    check('valid_schema5_reports', valid.length === entries.length && entries.length > 0, `${valid.length}/${entries.length}`, '100%'),
    check('exact_case_manifest', isDeepStrictEqual(unique(entries.map((entry) => entry.vars.caseId)).sort(), [...REQUIRED_CASE_IDS].sort()), unique(entries.map((entry) => entry.vars.caseId)).sort(), REQUIRED_CASE_IDS),
    check('required_case_sample_floor', requiredCaseMetrics.every((row) => row.runs >= row.minimum_runs), requiredCaseMetrics, 'at least the pinned per-case sample floor'),
    check('all_promptfoo_assertions_pass', entries.length > 0 && entries.every((entry) => entry.row.success === true), rate(entries, (entry) => entry.row.success === true), '100%'),
    check('single_cohort_boundary', boundaries.length === 1, boundaries.length, 1),
    check('exact_cohort_metadata', valid.length > 0 && valid.every(metadataIsExact), rate(valid, metadataIsExact), '100%'),
    check('clean_exact_source', valid.length > 0 && valid.every((entry) => entry.report.provenance?.source_dirty === false && entry.report.provenance?.build_source_dirty === false), rate(valid, (entry) => entry.report.provenance?.source_dirty === false && entry.report.provenance?.build_source_dirty === false), '100%'),
    check('gemma4_only', valid.length > 0 && valid.every((entry) => entry.report.requested_model === 'gemma4:12b-mlx' && entry.report.served_model === 'gemma4:12b-mlx'), rate(valid, (entry) => entry.report.requested_model === 'gemma4:12b-mlx' && entry.report.served_model === 'gemma4:12b-mlx'), '100%'),
    check('ordered_provenance', orderSequence, sortedOrders, 'unique contiguous order starting at 1'),
    check('single_concurrency_timeline', orderedRuns.length > 0 && nonOverlappingRuns, nonOverlappingRuns, 'non-overlapping run intervals'),
    check('oracle_isolation', valid.length > 0 && valid.every(oracleFree), rate(valid, oracleFree), '100%'),
    check('exact_case_aware_calls_per_turn', valid.length > 0 && valid.every(exactCallsPerTurn), rate(valid, exactCallsPerTurn), '100%'),
    check('known_recipe_sample_floor', knownCaseMetrics.length > 0 && knownCaseMetrics.every((row) => row.runs >= 10), knownCaseMetrics, 'at least 10 per known-recipe case'),
    check('known_recipe_selection', knownCaseMetrics.length > 0 && knownCaseMetrics.every((row) => row.selection_rate >= 0.9), knownCaseMetrics, 'at least 90% per known-recipe case'),
    check('selected_default_slice_gates_and_operations', selected.length > 0 && selected.every(receiptIsDefaultSlice), rate(selected, receiptIsDefaultSlice), '100%'),
    check('known_recipe_repeat_stability', knownStability.every((row) => row.pass), knownStability, 'one stable input, semantic, plan, and RuleSet identity per case'),
    check('per_case_route_adjudication_stability', routeAdjudication.every((row) => row.pass), routeAdjudication, 'one stable route semantic and adjudication identity per repeated case'),
    check('complete_request_question_rate', complete.length > 0 && complete.every(completeWithoutQuestion), 1 - (rate(complete, completeWithoutQuestion) ?? 0), '0%'),
    check('missing_decision_resolution', decisions.length > 0 && decisions.every(requiredDecisionResolved), rate(decisions, requiredDecisionResolved), '100%'),
    check('fallback_no_mutation_and_exact_route', fallbacks.length > 0 && [...new Set([...fallbacks, ...mutationProtected])].every(fallbackIsExact), rate([...new Set([...fallbacks, ...mutationProtected])], fallbackIsExact), '100%'),
    check('restart_continuity', restartCases.length > 0 && restartCases.every(restartIsExact), rate(restartCases, restartIsExact), '100%'),
    check('semantic_plan_ruleset_equivalence', equivalent.length > 0 && equivalent.every((group) => group.pass), equivalent, '100%'),
    check('semantic_to_ruleset_identity', semanticRuleset.pass, semanticRuleset, 'one canonical RuleSet identity for each semantic identity'),
    check('semantic_mutation_matrix', mutations.every((row) => row.pass), mutations, 'three stable runs with every identity distinct from the default for each mutation group'),
    check('one_call_preview_p50_latency', percentile(oneCallBuildTurnLatency, 0.5) !== null && percentile(oneCallBuildTurnLatency, 0.5) < 8000, percentile(oneCallBuildTurnLatency, 0.5), '<8000 ms'),
    check('one_call_preview_p95_latency', percentile(oneCallBuildTurnLatency, 0.95) !== null && percentile(oneCallBuildTurnLatency, 0.95) < 20000, percentile(oneCallBuildTurnLatency, 0.95), '<20000 ms'),
    check('two_call_preview_p50_latency', percentile(twoCallBuildTurnLatency, 0.5) !== null && percentile(twoCallBuildTurnLatency, 0.5) <= 22000, percentile(twoCallBuildTurnLatency, 0.5), '<=22000 ms'),
    check('two_call_preview_p95_latency', percentile(twoCallBuildTurnLatency, 0.95) !== null && percentile(twoCallBuildTurnLatency, 0.95) <= 30000, percentile(twoCallBuildTurnLatency, 0.95), '<=30000 ms'),
    check('interactive_hard_limit', allTurnLatency.length > 0 && allTurnLatency.every((elapsed) => elapsed <= 60000), allTurnLatency.length === 0 ? null : Math.max(...allTurnLatency), '<=60000 ms'),
  ];
  const starts = finite(valid.map((entry) => Number(entry.report.provenance?.started_at_unix_ms)));
  const ends = finite(valid.map((entry) => Number(entry.report.provenance?.ended_at_unix_ms)));
  return {
    pass: checks.every((entry) => entry.pass),
    cohort: 'intent_recipe',
    scope: 'private_study_room_intent_v4_checkpoint',
    samples: entries.length,
    valid_reports: valid.length,
    boundary: boundaries.length === 1 ? boundaries[0] : null,
    started_at_unix_ms: starts.length === 0 ? null : Math.min(...starts),
    ended_at_unix_ms: ends.length === 0 ? null : Math.max(...ends),
    p50_one_call_preview_turn_ms: percentile(oneCallBuildTurnLatency, 0.5),
    p95_one_call_preview_turn_ms: percentile(oneCallBuildTurnLatency, 0.95),
    p50_two_call_preview_turn_ms: percentile(twoCallBuildTurnLatency, 0.5),
    p95_two_call_preview_turn_ms: percentile(twoCallBuildTurnLatency, 0.95),
    known_recipe_cases: knownCaseMetrics,
    required_cases: requiredCaseMetrics,
    known_recipe_stability: knownStability,
    route_adjudication_stability: routeAdjudication,
    equivalence_groups: equivalent,
    semantic_ruleset_identity: semanticRuleset,
    mutation_groups: mutations,
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
