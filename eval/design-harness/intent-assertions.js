function object(value, location) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`invalid intent eval report: missing object ${location}`);
  }
  return value;
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

function sameJson(left, right) {
  return JSON.stringify(stable(left)) === JSON.stringify(stable(right));
}

function integer(value, location) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`invalid intent eval report: ${location} must be a non-negative integer`);
  }
  return value;
}

function parseReport(output) {
  let report;
  try {
    report = typeof output === 'string' ? JSON.parse(output) : output;
  } catch (error) {
    throw new Error(`invalid intent eval report: ${error.message}`);
  }
  object(report, 'report');
  if (report.schema_version !== 3 || report.input_schema_version !== 3) {
    throw new Error('invalid intent eval report: schema_version and input_schema_version must be 3');
  }
  if (report.mode !== 'intent_recipe') {
    throw new Error('invalid intent eval report: mode must be intent_recipe');
  }
  if (!Array.isArray(report.turns) || report.turns.length === 0) {
    throw new Error('invalid intent eval report: turns must be a non-empty array');
  }
  if (typeof report.outcome !== 'string' || typeof report.completed !== 'boolean') {
    throw new Error('invalid intent eval report: terminal outcome and completion are required');
  }
  object(report.ruleset, 'ruleset');
  object(report.actual_gates, 'actual_gates');
  object(report.observability, 'observability');
  object(report.provenance, 'provenance');
  object(report.oracle, 'oracle');
  object(report.session_config, 'session_config');
  object(report.final_intent, 'final_intent');
  object(report.final_intent.public_status, 'final_intent.public_status');
  object(report.persistence, 'persistence');
  if (typeof report.final_intent.status !== 'string'
    || typeof report.final_intent.public_status.status !== 'string') {
    throw new Error('invalid intent eval report: final intent status is required');
  }
  if (!/^[0-9a-f]{64}$/.test(report.final_intent.binding_fingerprint)) {
    throw new Error('invalid intent eval report: binding fingerprint must be a SHA-256 hash');
  }
  integer(report.draft_revision, 'draft_revision');
  integer(report.elapsed_ms, 'elapsed_ms');
  integer(report.persistence.store_writes, 'persistence.store_writes');
  integer(report.persistence.connection_reopen_count, 'persistence.connection_reopen_count');
  integer(report.persistence.final_generation, 'persistence.final_generation');
  integer(report.persistence.snapshot_schema_version, 'persistence.snapshot_schema_version');
  if (report.persistence.backend !== 'sqlite_file'
    || typeof report.persistence.roundtrip_verified !== 'boolean') {
    throw new Error('invalid intent eval report: SQLite persistence evidence is required');
  }
  for (const [index, turn] of report.turns.entries()) {
    object(turn, `turns[${index}]`);
    object(turn.intent_counters, `turns[${index}].intent_counters`);
    object(turn.intent_counters.fallback_routes, `turns[${index}].intent_counters.fallback_routes`);
    object(turn.actual_gates, `turns[${index}].actual_gates`);
    if (typeof turn.id !== 'string'
      || typeof turn.outcome !== 'string'
      || typeof turn.stage_before !== 'string'
      || typeof turn.stage_after !== 'string'
      || typeof turn.draft_changed !== 'boolean'
      || typeof turn.restart_after !== 'boolean'
      || typeof turn.restart_performed !== 'boolean') {
      throw new Error(`invalid intent eval report: turns[${index}] has invalid lifecycle fields`);
    }
    for (const field of ['model_calls', 'model_tool_calls', 'deterministic_operations', 'elapsed_ms']) {
      integer(turn[field], `turns[${index}].${field}`);
    }
    for (const field of [
      'intent_revision_before',
      'intent_revision_after',
      'draft_revision_before',
      'draft_revision_after',
    ]) {
      integer(turn[field], `turns[${index}].${field}`);
    }
    for (const field of [
      'route_calls',
      'proposal_acceptances',
      'resolution_acceptances',
      'compile_attempts',
      'compile_successes',
      'commits',
      'rollbacks',
      'conflicts',
      'stale_revision_rejections',
      'extraction_failures',
    ]) {
      integer(turn.intent_counters[field], `turns[${index}].intent_counters.${field}`);
    }
    for (const [kind, count] of Object.entries(turn.intent_counters.fallback_routes)) {
      integer(count, `turns[${index}].intent_counters.fallback_routes.${kind}`);
    }
  }
  return report;
}

function vars(context) {
  return context?.vars || context?.test?.vars || {};
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

function result(pass, reason, score = pass ? 1 : 0) {
  return { pass, score, reason };
}

function checked(output, assertion) {
  try {
    return assertion(parseReport(output));
  } catch (error) {
    return result(false, error.message);
  }
}

function hashesAreValid(receipt) {
  return ['input_intent_hash', 'semantic_intent_hash', 'compiled_plan_hash']
    .every((field) => /^[0-9a-f]{64}$/.test(receipt[field]));
}

function intentProvenance(output) {
  return checked(output, (report) => {
    const failures = [];
    if (report.requested_model !== 'gemma4:12b-mlx') {
      failures.push(`requested_model=${report.requested_model}`);
    }
    if (report.served_model !== 'gemma4:12b-mlx') {
      failures.push(`served_model=${report.served_model}`);
    }
    if (!/^sha256-[0-9a-f]{64}$/.test(report.gateway_id)) {
      failures.push('gateway_id is not an opaque SHA-256 identity');
    }
    if (report.declared_context_tokens !== 16384
      || report.context_declaration_source !== 'evaluation_provider'
      || report.gateway_context_observed_tokens !== null) {
      failures.push(`declared_context_tokens=${report.declared_context_tokens}`);
    }
    const provenance = report.provenance;
    if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(provenance.source_commit)) {
      failures.push('source_commit is not exact');
    }
    if (provenance.source_dirty !== false) {
      failures.push(`source_dirty=${provenance.source_dirty}`);
    }
    if (provenance.build_source_commit !== provenance.source_commit
      || provenance.build_source_dirty !== false) {
      failures.push('build source attestation does not match the clean runner source');
    }
    if (!/^[0-9a-f]{64}$/.test(provenance.binary_sha256)) {
      failures.push('binary_sha256 is not exact');
    }
    if (provenance.attestation_kind !== 'local_unsigned') {
      failures.push(`attestation_kind=${provenance.attestation_kind}`);
    }
    if (typeof provenance.run_id !== 'string' || provenance.run_id.length === 0) {
      failures.push('run_id is missing');
    }
    if (!Number.isSafeInteger(provenance.run_order) || provenance.run_order < 1) {
      failures.push(`run_order=${provenance.run_order}`);
    }
    if (!Number.isSafeInteger(provenance.started_at_unix_ms)
      || !Number.isSafeInteger(provenance.ended_at_unix_ms)
      || provenance.ended_at_unix_ms < provenance.started_at_unix_ms) {
      failures.push('provenance timestamps are invalid');
    }
    if (!sameJson(report.session_config, {
      max_model_calls: 12,
      max_tool_calls: 24,
      max_gate_failures: 4,
      context_char_budget: 44000,
    })) {
      failures.push('session_config differs from the pinned benchmark policy');
    }
    return result(failures.length === 0, failures.length === 0
      ? 'Gemma4 cohort source, binary, and declared context policy are exact and clean'
      : failures.join(', '));
  });
}

function intentRouteStage(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const outcomes = list(expected.expectedOutcomes);
    const stages = list(expected.expectedStagePath);
    const routes = list(expected.expectedRoutePath);
    const failures = [];
    if (outcomes.length > 0 && outcomes.length !== report.turns.length) {
      failures.push(`outcome path length=${report.turns.length} expected=${outcomes.length}`);
    }
    if (stages.length > 0 && stages.length !== report.turns.length) {
      failures.push(`stage path length=${report.turns.length} expected=${stages.length}`);
    }
    if (routes.length > 0 && routes.length !== report.turns.length) {
      failures.push(`route path length=${report.turns.length} expected=${routes.length}`);
    }
    for (const [index, turn] of report.turns.entries()) {
      if (outcomes[index] && turn.outcome !== outcomes[index]) {
        failures.push(`${turn.id} outcome=${turn.outcome} expected=${outcomes[index]}`);
      }
      const actualStage = `${turn.stage_before}>${turn.stage_after}`;
      if (stages[index] && actualStage !== stages[index]) {
        failures.push(`${turn.id} stage=${actualStage} expected=${stages[index]}`);
      }
      const route = routes[index];
      const counters = turn.intent_counters;
      const fallbacks = Object.entries(counters.fallback_routes).filter(([, count]) => count > 0);
      if (route === 'private_study_room') {
        if (counters.route_calls !== 1 || counters.proposal_acceptances !== 1 || fallbacks.length !== 0) {
          failures.push(`${turn.id} did not accept exactly one private_study_room route`);
        }
      } else if (route === 'resolve_intent_decision') {
        if (counters.route_calls !== 0 || counters.resolution_acceptances !== 1 || fallbacks.length !== 0) {
          failures.push(`${turn.id} did not accept exactly one intent decision`);
        }
      } else if (route) {
        if (counters.route_calls !== 1
          || counters.proposal_acceptances !== 0
          || fallbacks.length !== 1
          || fallbacks[0][0] !== route
          || fallbacks[0][1] !== 1) {
          failures.push(`${turn.id} fallback=${JSON.stringify(counters.fallback_routes)} expected=${route}`);
        }
      }
      if (counters.extraction_failures !== 0
        || counters.stale_revision_rejections !== 0
        || counters.rollbacks !== 0
        || counters.conflicts !== 0) {
        failures.push(`${turn.id} recorded extraction, stale, rollback, or conflict failures`);
      }
    }
    if (typeof expected.expectedFinalStatus === 'string'
      && report.final_intent.status !== expected.expectedFinalStatus) {
      failures.push(`final status=${report.final_intent.status} expected=${expected.expectedFinalStatus}`);
    }
    if (report.final_intent.public_status.status !== report.final_intent.status) {
      failures.push(`public status=${report.final_intent.public_status.status} final status=${report.final_intent.status}`);
    }
    return result(failures.length === 0, failures.length === 0
      ? 'intent routes and durable stages match'
      : failures.join(', '));
  });
}

function intentReceipt(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const expectedOperations = expected.expectedCompiledOperations;
    const receipt = report.final_intent.receipt;
    const failures = [];
    if (report.final_intent.status === 'preview_ready') {
      if (!receipt || typeof receipt !== 'object' || Array.isArray(receipt)) {
        failures.push('preview_ready status has no receipt');
      } else {
        if (!hashesAreValid(receipt)) {
          failures.push('receipt hashes are invalid');
        }
        if (Number.isInteger(expectedOperations) && receipt.compiled_operations !== expectedOperations) {
          failures.push(`compiled_operations=${receipt.compiled_operations} expected=${expectedOperations}`);
        }
        if (receipt.candidate_revision !== report.draft_revision) {
          failures.push(`candidate_revision=${receipt.candidate_revision} draft_revision=${report.draft_revision}`);
        }
        if (!sameJson(report.final_intent.public_status.receipt, receipt)) {
          failures.push('public status receipt differs from the top-level intent receipt');
        }
      }
      if (report.actual_gates.validation_current !== true || report.actual_gates.simulation_current !== true) {
        failures.push('preview receipt is missing current validation or simulation stamps');
      }
      if (report.completed !== true || report.outcome !== 'ready') {
        failures.push(`preview terminal outcome=${report.outcome} completed=${report.completed}`);
      }
    } else {
      if (receipt !== null) {
        failures.push(`non-preview status leaked receipt=${JSON.stringify(receipt)}`);
      }
      if (Number.isInteger(expectedOperations)) {
        failures.push(`expected ${expectedOperations} compiled operations without a preview`);
      }
    }
    return result(failures.length === 0, failures.length === 0
      ? 'recipe receipt and binding gates are valid'
      : failures.join(', '));
  });
}

function intentOneCallTurns(output) {
  return checked(output, (report) => {
    const failures = [];
    for (const turn of report.turns) {
      if (turn.model_calls !== 1 || turn.model_tool_calls !== 1) {
        failures.push(`${turn.id} calls=${turn.model_calls}/${turn.model_tool_calls} expected=1/1`);
      }
    }
    const modelCalls = report.turns.reduce((sum, turn) => sum + turn.model_calls, 0);
    const modelToolCalls = report.turns.reduce((sum, turn) => sum + turn.model_tool_calls, 0);
    if (report.observability.model_calls !== modelCalls) {
      failures.push(`cumulative model_calls=${report.observability.model_calls} expected=${modelCalls}`);
    }
    if (report.observability.tool_calls !== modelToolCalls) {
      failures.push(`cumulative tool_calls=${report.observability.tool_calls} expected=${modelToolCalls}`);
    }
    return result(failures.length === 0, failures.length === 0
      ? 'every ordinary turn used one model call and one frontier call'
      : failures.join(', '));
  });
}

function intentOracleIsolation(output) {
  return checked(output, (report) => {
    const failures = [];
    if (report.oracle.enabled !== false || report.oracle.injected_control_calls !== 0) {
      failures.push(`oracle=${JSON.stringify(report.oracle)}`);
    }
    if (Number(report.injected_control_calls ?? 0) !== 0) {
      failures.push(`legacy injected_control_calls=${report.injected_control_calls}`);
    }
    for (const turn of report.turns) {
      if (Number(turn.injected_control_calls ?? 0) !== 0) {
        failures.push(`${turn.id} injected controls`);
      }
    }
    return result(failures.length === 0, failures.length === 0
      ? 'intent cohort is oracle-free'
      : failures.join(', '));
  });
}

function intentDecisionFlow(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const failures = [];
    const decisionTurns = report.turns.filter((turn) => turn.stage_after === 'awaiting_decision');
    const questionTurns = report.turns.filter((turn) => typeof turn.question === 'string' && turn.question.length > 0);
    if (expected.requiresDecision === true) {
      if (decisionTurns.length !== 1 || questionTurns.length !== 1) {
        failures.push(`decision turns=${decisionTurns.length} questions=${questionTurns.length} expected=1/1`);
      }
      const pending = decisionTurns[0];
      if (pending && (pending.outcome !== 'needs_input'
        || pending.draft_changed !== false
        || pending.draft_revision_before !== pending.draft_revision_after)) {
        failures.push('blocking decision turn mutated the Draft or returned the wrong outcome');
      }
      const resolutions = report.turns.reduce(
        (sum, turn) => sum + turn.intent_counters.resolution_acceptances,
        0,
      );
      if (resolutions !== 1) {
        failures.push(`resolution_acceptances=${resolutions} expected=1`);
      }
    }
    if (expected.completeRequest === true && (decisionTurns.length !== 0 || questionTurns.length !== 0)) {
      failures.push('complete request asked an unnecessary question');
    }
    return result(failures.length === 0, failures.length === 0
      ? 'deterministic decision behavior matches the request'
      : failures.join(', '));
  });
}

function intentRestartContinuity(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const expectedRestarts = Number.isInteger(expected.expectedRestartCount)
      ? expected.expectedRestartCount
      : 0;
    const failures = [];
    if (report.persistence.connection_reopen_count !== expectedRestarts) {
      failures.push(`connection_reopen_count=${report.persistence.connection_reopen_count} expected=${expectedRestarts}`);
    }
    if (report.persistence.store_writes !== report.turns.length
      || report.persistence.final_generation !== report.turns.length) {
      failures.push('every completed turn must be saved through SQLite generation CAS');
    }
    if (expectedRestarts > 0 && report.persistence.roundtrip_verified !== true) {
      failures.push('requested SQLite close/reopen roundtrip was not verified');
    }
    const performed = report.turns.filter((turn) => turn.restart_performed === true);
    if (performed.length !== expectedRestarts) {
      failures.push(`restart markers=${performed.length} expected=${expectedRestarts}`);
    }
    for (let index = 1; index < report.turns.length; index += 1) {
      const before = report.turns[index - 1];
      const after = report.turns[index];
      if (before.draft_revision_after !== after.draft_revision_before) {
        failures.push(`draft revision discontinuity between ${before.id} and ${after.id}`);
      }
      if (before.restart_performed === true) {
        if (before.restart_after !== true
          || before.stage_after !== after.stage_before
          || before.intent_revision_after !== after.intent_revision_before) {
          failures.push(`intent state did not survive restart after ${before.id}`);
        }
      }
    }
    return result(failures.length === 0, failures.length === 0
      ? 'restart and revision continuity are durable'
      : failures.join(', '));
  });
}

function intentNoMutationFallback(output, context) {
  return checked(output, (report) => {
    const ids = new Set(list(vars(context).noMutationTurns));
    const failures = [];
    for (const turn of report.turns.filter((entry) => ids.has(entry.id))) {
      const counters = turn.intent_counters;
      if (turn.outcome !== 'routed'
        || turn.draft_changed !== false
        || turn.draft_revision_before !== turn.draft_revision_after
        || turn.deterministic_operations !== 0
        || counters.compile_attempts !== 0
        || counters.compile_successes !== 0
        || counters.commits !== 0
        || counters.rollbacks !== 0
        || counters.conflicts !== 0) {
        failures.push(`${turn.id} fallback mutated or compiled the canonical Draft`);
      }
    }
    if (ids.size > 0 && report.turns.filter((entry) => ids.has(entry.id)).length !== ids.size) {
      failures.push('one or more expected no-mutation turns were not reported');
    }
    return result(failures.length === 0, failures.length === 0
      ? 'fallback turns are mutation-free'
      : failures.join(', '));
  });
}

function intentHardLatency(output) {
  return checked(output, (report) => {
    const overLimit = report.turns.filter((turn) => turn.elapsed_ms > 60000);
    return result(overLimit.length === 0, overLimit.length === 0
      ? 'every turn stayed within the 60 second safe boundary'
      : `turns exceeded 60 seconds: ${overLimit.map((turn) => turn.id).join(', ')}`);
  });
}

module.exports = {
  intentDecisionFlow,
  intentHardLatency,
  intentNoMutationFallback,
  intentOneCallTurns,
  intentOracleIsolation,
  intentProvenance,
  intentReceipt,
  intentRestartContinuity,
  intentRouteStage,
  parseReport,
};
