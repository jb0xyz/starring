const { createHash } = require('node:crypto');
const {
  INTENT_EXTRACTOR_REVISION,
  INTENT_NORMALIZER_REVISION,
  INTENT_REGISTRY_DIGEST,
} = require('./catalog-identity');

const INTENT_MANIFEST_DIGEST = '68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53';
const INTENT_PROTOCOL_VERSION = 4;
const INTENT_ADJUDICATOR_VERSION = 3;
const INTENT_IDENTITY_REVISION = 2;
const INTENT_SNAPSHOT_VERSION = 8;
const SHA256 = /^[0-9a-f]{64}$/;
const DECISION_KINDS = new Set([
  'private_study_room',
  'typed_planner',
  'capability_gap',
  'reject',
  'discussion',
]);
const CAPABILITY_CONTRACTS = {
  durable_timer: ['unavailable', null],
  event_time_llm_decision: ['forbidden_policy', 'event_time_llm_execution_forbidden_v1'],
  instance_creator_teardown_authorization: ['unavailable', null],
  persistent_economy_ledger: ['unavailable', null],
  restart_persistent_state: ['unavailable', null],
  unclassified_intent_requirement: ['unclassified', null],
};
const CAPABILITY_EVIDENCE_CONTRACTS = {
  durable_timer: [{
    semantic_path: 'intent.core.runtime_requirements.timers',
    description: 'durable',
  }],
  event_time_llm_decision: [{
    semantic_path: 'intent.core.runtime_requirements.event_time_llm',
    description: 'true',
  }],
  persistent_economy_ledger: [{
    semantic_path: 'intent.core.runtime_requirements.economy',
    description: 'persistent_ledger',
  }],
  restart_persistent_state: [{
    semantic_path: 'intent.core.runtime_requirements.persistence',
    description: 'restart_persistent',
  }],
};
const BOUNDARY_IDS = new Set([
  'bypass_validation_preview_approval',
  'direct_live_mutation',
  'secret_disclosure',
]);
const CAPABILITY_LABELS = {
  durable_timer: ['Durable timers', '영속 타이머'],
  event_time_llm_decision: ['Event-time LLM decisions', '이벤트 시점 LLM 결정'],
  instance_creator_teardown_authorization: [
    'Creator-only room teardown authorization',
    '방 생성자 전용 종료 권한',
  ],
  persistent_economy_ledger: ['Persistent economy ledger', '영속 경제 원장'],
  restart_persistent_state: ['State preserved across restarts', '재시작 후에도 보존되는 상태'],
  unclassified_intent_requirement: ['Unclassified hard requirement', '분류되지 않은 필수 요구사항'],
};
const CAPABILITY_STATUSES = {
  available: ['available', '사용 가능'],
  unavailable: ['unavailable', '사용 불가'],
  forbidden_policy: ['forbidden by policy', '정책상 금지'],
  unclassified: ['unclassified', '미분류'],
};
const BOUNDARY_LABELS = {
  bypass_validation_preview_approval: [
    'Bypass validation, preview, and approval',
    '검증, 미리보기, 승인 우회',
  ],
  direct_live_mutation: ['Direct live mutation', '직접 라이브 변경'],
  secret_disclosure: ['Secret disclosure', '비밀정보 노출'],
};
const DISCUSSION_MAX_UTF16_UNITS = 480;
const DISCUSSION_MAX_SENTENCES = 4;
const DISCUSSION_MAX_LIST_ITEMS = 4;

function object(value, location) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`invalid intent eval report: missing object ${location}`);
  }
  return value;
}

function exactKeys(value, keys, location) {
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  if (!sameJson(actual, expected)) {
    throw new Error(`invalid intent eval report: ${location} has invalid fields`);
  }
}

function nonEmptyString(value, location) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`invalid intent eval report: ${location} must be a non-empty string`);
  }
  return value;
}

function sortedUnique(values, location) {
  const sorted = [...values].sort();
  if (!sameJson(values, sorted) || new Set(values).size !== values.length) {
    throw new Error(`invalid intent eval report: ${location} must be sorted and unique`);
  }
}

function evidence(value, location) {
  object(value, location);
  exactKeys(value, ['semantic_path', 'description'], location);
  nonEmptyString(value.semantic_path, `${location}.semantic_path`);
  nonEmptyString(value.description, `${location}.description`);
  return value;
}

function evidenceList(value, location) {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`invalid intent eval report: ${location} must be a non-empty array`);
  }
  value.forEach((entry, index) => evidence(entry, `${location}[${index}]`));
  const identities = value.map((entry) => `${entry.semantic_path}\u0000${entry.description}`);
  sortedUnique(identities, location);
}

function routeDecision(value, location) {
  if (value === null) {
    return null;
  }
  object(value, location);
  exactKeys(value, [
    'kind',
    'decision_source',
    'adjudicator_version',
    'semantic_ir_digest',
    'request_evidence_hash',
    'manifest_version',
    'manifest_digest',
    'adjudication_digest',
    'blockers',
    'boundary_violations',
    'unclassified_requirements',
    'route_target',
  ], location);
  if (!DECISION_KINDS.has(value.kind)) {
    throw new Error(`invalid intent eval report: ${location}.kind is invalid`);
  }
  if (value.decision_source !== 'deterministic_intent_adjudicator'
    || value.adjudicator_version !== INTENT_ADJUDICATOR_VERSION) {
    throw new Error(`invalid intent eval report: ${location} has invalid adjudicator identity`);
  }
  if (!SHA256.test(value.semantic_ir_digest)
    || !SHA256.test(value.request_evidence_hash)
    || !SHA256.test(value.adjudication_digest)) {
    throw new Error(`invalid intent eval report: ${location} has invalid decision hashes`);
  }
  if (value.manifest_version !== 1 || value.manifest_digest !== INTENT_MANIFEST_DIGEST) {
    throw new Error(`invalid intent eval report: ${location} has invalid capability manifest identity`);
  }
  if (!Array.isArray(value.blockers)) {
    throw new Error(`invalid intent eval report: ${location}.blockers must be an array`);
  }
  for (const [index, blocker] of value.blockers.entries()) {
    const blockerLocation = `${location}.blockers[${index}]`;
    object(blocker, blockerLocation);
    exactKeys(blocker, ['id', 'status', 'policy_id', 'evidence'], blockerLocation);
    const contract = CAPABILITY_CONTRACTS[blocker.id];
    if (!contract || blocker.status !== contract[0] || blocker.policy_id !== contract[1]) {
      throw new Error(`invalid intent eval report: ${blockerLocation} contradicts the capability manifest`);
    }
    evidenceList(blocker.evidence, `${blockerLocation}.evidence`);
    const evidenceContract = CAPABILITY_EVIDENCE_CONTRACTS[blocker.id];
    if (evidenceContract && !sameJson(blocker.evidence, evidenceContract)) {
      throw new Error(`invalid intent eval report: ${blockerLocation}.evidence contradicts the capability evidence contract`);
    }
  }
  sortedUnique(value.blockers.map((blocker) => blocker.id), `${location}.blockers`);
  if (!Array.isArray(value.boundary_violations)) {
    throw new Error(`invalid intent eval report: ${location}.boundary_violations must be an array`);
  }
  for (const [index, violation] of value.boundary_violations.entries()) {
    const violationLocation = `${location}.boundary_violations[${index}]`;
    object(violation, violationLocation);
    exactKeys(violation, ['id', 'evidence'], violationLocation);
    if (!BOUNDARY_IDS.has(violation.id)) {
      throw new Error(`invalid intent eval report: ${violationLocation}.id is invalid`);
    }
    evidenceList(violation.evidence, `${violationLocation}.evidence`);
  }
  sortedUnique(
    value.boundary_violations.map((violation) => violation.id),
    `${location}.boundary_violations`,
  );
  if (!Array.isArray(value.unclassified_requirements)) {
    throw new Error(`invalid intent eval report: ${location}.unclassified_requirements must be an array`);
  }
  value.unclassified_requirements.forEach((entry, index) => {
    nonEmptyString(entry, `${location}.unclassified_requirements[${index}]`);
  });
  sortedUnique(value.unclassified_requirements, `${location}.unclassified_requirements`);
  const unclassifiedBlocker = value.blockers.find(
    (blocker) => blocker.id === 'unclassified_intent_requirement',
  );
  const expectedUnclassifiedEvidence = value.unclassified_requirements.map(
    (description, index) => ({
      semantic_path: `intent.core.unclassified_requirements.${index}`,
      description,
    }),
  );
  const actualUnclassifiedEvidence = unclassifiedBlocker?.evidence ?? [];
  if (!sameJson(actualUnclassifiedEvidence, expectedUnclassifiedEvidence)) {
    throw new Error(`invalid intent eval report: ${location} unclassified evidence does not match indexed unclassified_requirements`);
  }
  if (value.route_target === null) {
    if (value.kind === 'private_study_room') {
      throw new Error(`invalid intent eval report: ${location} is missing its pinned recipe`);
    }
  } else {
    object(value.route_target, `${location}.route_target`);
    exactKeys(value.route_target, ['recipe_id', 'recipe_version'], `${location}.route_target`);
    if (value.kind !== 'private_study_room'
      || value.route_target.recipe_id !== 'starring.private_study_room'
      || value.route_target.recipe_version !== 1) {
      throw new Error(`invalid intent eval report: ${location}.route_target is invalid`);
    }
  }
  if (value.kind === 'capability_gap'
    && (value.blockers.length === 0 || value.boundary_violations.length !== 0)) {
    throw new Error(`invalid intent eval report: ${location} has an invalid capability-gap shape`);
  }
  if (value.kind === 'reject' && value.boundary_violations.length === 0) {
    throw new Error(`invalid intent eval report: ${location} has an invalid reject shape`);
  }
  if (['private_study_room', 'typed_planner', 'discussion'].includes(value.kind)
    && (value.blockers.length !== 0
      || value.boundary_violations.length !== 0
      || value.unclassified_requirements.length !== 0)) {
    throw new Error(`invalid intent eval report: ${location} has blockers on a non-blocking route`);
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

function domainSeparatedCanonicalDigest(domain, value) {
  return createHash('sha256')
    .update(domain)
    .update(JSON.stringify(stable(value)))
    .digest('hex');
}

function candidateIdentityHashes(report) {
  const draft = {
    ruleset: report.ruleset,
    draft_revision: report.draft_revision,
    validated_revision: report.actual_gates.validated_revision,
    simulated_revision: report.actual_gates.simulated_revision,
  };
  return {
    candidate_ruleset_hash: domainSeparatedCanonicalDigest(
      'starring.intent.candidate_ruleset.v1\0',
      report.ruleset,
    ),
    candidate_draft_hash: domainSeparatedCanonicalDigest(
      'starring.intent.draft_state.v1\0',
      draft,
    ),
  };
}

function integer(value, location) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`invalid intent eval report: ${location} must be a non-negative integer`);
  }
  return value;
}

function receiptValue(value, location) {
  object(value, location);
  exactKeys(value, [
    'identity_revision',
    'intent_revision',
    'candidate_revision',
    'request_evidence_hash',
    'request_evidence_entries',
    'compiler_input_hash',
    'semantic_intent_hash',
    'compiled_plan_hash',
    'candidate_ruleset_hash',
    'candidate_draft_hash',
    'compiled_operations',
  ], location);
  if (value.identity_revision !== INTENT_IDENTITY_REVISION) {
    throw new Error(`invalid intent eval report: ${location}.identity_revision is invalid`);
  }
  for (const field of [
    'request_evidence_hash',
    'compiler_input_hash',
    'semantic_intent_hash',
    'compiled_plan_hash',
    'candidate_ruleset_hash',
    'candidate_draft_hash',
  ]) {
    if (!SHA256.test(value[field])) {
      throw new Error(`invalid intent eval report: ${location}.${field} must be a SHA-256 hash`);
    }
  }
  for (const field of [
    'intent_revision',
    'candidate_revision',
    'request_evidence_entries',
    'compiled_operations',
  ]) {
    integer(value[field], `${location}.${field}`);
  }
  if (value.request_evidence_entries < 1) {
    throw new Error(`invalid intent eval report: ${location}.request_evidence_entries must be positive`);
  }
  return value;
}

function publicStatus(value, location) {
  object(value, location);
  if (value.status === 'empty') {
    exactKeys(value, ['status', 'expected_revision'], location);
    integer(value.expected_revision, `${location}.expected_revision`);
    return value;
  }
  if (value.status === 'awaiting_decision') {
    exactKeys(value, [
      'status',
      'root_draft_revision',
      'workspace_revision',
      'question',
      'available_channel_keys',
    ], location);
    integer(value.root_draft_revision, `${location}.root_draft_revision`);
    integer(value.workspace_revision, `${location}.workspace_revision`);
    nonEmptyString(value.question, `${location}.question`);
    if (!Array.isArray(value.available_channel_keys)
      || value.available_channel_keys.some((entry) => typeof entry !== 'string')) {
      throw new Error(`invalid intent eval report: ${location}.available_channel_keys must be strings`);
    }
    sortedUnique(value.available_channel_keys, `${location}.available_channel_keys`);
    return value;
  }
  if (value.status === 'preview_ready') {
    exactKeys(value, [
      'status',
      'root_draft_revision',
      'workspace_revision',
      'receipt',
    ], location);
    integer(value.root_draft_revision, `${location}.root_draft_revision`);
    integer(value.workspace_revision, `${location}.workspace_revision`);
    receiptValue(value.receipt, `${location}.receipt`);
    return value;
  }
  throw new Error(`invalid intent eval report: ${location}.status is invalid`);
}

function actualGates(value, location, draftRevision) {
  object(value, location);
  exactKeys(value, [
    'validated_revision',
    'simulated_revision',
    'validation_current',
    'simulation_current',
  ], location);
  for (const field of ['validated_revision', 'simulated_revision']) {
    if (value[field] !== null) {
      integer(value[field], `${location}.${field}`);
    }
  }
  if (typeof value.validation_current !== 'boolean'
    || typeof value.simulation_current !== 'boolean') {
    throw new Error(`invalid intent eval report: ${location} current flags must be booleans`);
  }
  if (value.validation_current !== (value.validated_revision === draftRevision)
    || value.simulation_current !== (value.simulated_revision === draftRevision)) {
    throw new Error(`invalid intent eval report: ${location} revision stamps are inconsistent`);
  }
  return value;
}

function modelCallMetric(value, location) {
  object(value, location);
  exactKeys(value, [
    'call_sequence',
    'attempt',
    'frontier_name',
    'outcome',
    'http_status',
    'served_model',
    'request_body_bytes',
    'message_bytes',
    'tool_bytes',
    'duplicated_schema_bytes',
    'prompt_tokens',
    'completion_tokens',
    'finish_reason',
    'request_duration_ms',
    'gateway_model_duration_ms',
  ], location);
  nonEmptyString(value.frontier_name, `${location}.frontier_name`);
  for (const field of [
    'call_sequence',
    'attempt',
    'request_body_bytes',
    'message_bytes',
    'tool_bytes',
    'duplicated_schema_bytes',
    'request_duration_ms',
  ]) {
    integer(value[field], `${location}.${field}`);
  }
  if (value.call_sequence === 0 || value.attempt === 0) {
    throw new Error(`invalid intent eval report: ${location} call and attempt indexes must be positive`);
  }
  for (const field of ['prompt_tokens', 'completion_tokens']) {
    if (value[field] !== null) {
      integer(value[field], `${location}.${field}`);
    }
  }
  if (value.finish_reason !== null) {
    nonEmptyString(value.finish_reason, `${location}.finish_reason`);
  }
  if (value.gateway_model_duration_ms !== null) {
    integer(value.gateway_model_duration_ms, `${location}.gateway_model_duration_ms`);
  }
  const outcomes = [
    'succeeded',
    'transport_error',
    'http_error',
    'response_body_error',
    'malformed_json',
    'invalid_response',
  ];
  if (!outcomes.includes(value.outcome)) {
    throw new Error(`invalid intent eval report: ${location}.outcome is invalid`);
  }
  if (value.http_status !== null) {
    integer(value.http_status, `${location}.http_status`);
    if (value.http_status < 100 || value.http_status > 599) {
      throw new Error(`invalid intent eval report: ${location}.http_status is invalid`);
    }
  }
  if (value.served_model !== null && typeof value.served_model !== 'string') {
    throw new Error(`invalid intent eval report: ${location}.served_model must be a string or null`);
  }
  const successfulHttp = value.http_status !== null
    && value.http_status >= 200
    && value.http_status < 300;
  if (value.outcome === 'transport_error' && value.http_status !== null) {
    throw new Error(`invalid intent eval report: ${location} transport errors cannot have HTTP status`);
  }
  if (value.outcome === 'http_error' && (value.http_status === null || successfulHttp)) {
    throw new Error(`invalid intent eval report: ${location} HTTP error status is inconsistent`);
  }
  if (['succeeded', 'response_body_error', 'malformed_json', 'invalid_response']
    .includes(value.outcome) && !successfulHttp) {
    throw new Error(`invalid intent eval report: ${location} response outcome lacks successful HTTP status`);
  }
  if (value.outcome === 'succeeded'
    && (typeof value.served_model !== 'string' || value.served_model.length === 0)) {
    throw new Error(`invalid intent eval report: ${location} successful response lacks model provenance`);
  }
  if (!successfulHttp && value.served_model !== null) {
    throw new Error(`invalid intent eval report: ${location} non-successful HTTP attempt has model provenance`);
  }
  if (['transport_error', 'http_error', 'response_body_error', 'malformed_json']
    .includes(value.outcome)
    && (value.prompt_tokens !== null || value.completion_tokens !== null)) {
    throw new Error(`invalid intent eval report: ${location} failed response has fabricated token usage`);
  }
  if (value.outcome !== 'succeeded' && value.finish_reason !== null) {
    throw new Error(`invalid intent eval report: ${location} failed response has a finish reason`);
  }
  if (value.request_body_bytes === 0
    || value.message_bytes === 0
    || value.tool_bytes === 0
    || value.request_body_bytes
      <= value.message_bytes + value.tool_bytes + value.duplicated_schema_bytes) {
    throw new Error(`invalid intent eval report: ${location} byte accounting is invalid`);
  }
  if (value.frontier_name === 'interpret_intent_core'
    && (value.duplicated_schema_bytes > 1600
      || value.tool_bytes + value.duplicated_schema_bytes > 3800)) {
    throw new Error(`invalid intent eval report: ${location} exceeds the Core schema budget`);
  }
  if (value.frontier_name === 'extract_private_study_room_details'
    && value.duplicated_schema_bytes >= 2100) {
    throw new Error(`invalid intent eval report: ${location} exceeds the detail schema budget`);
  }
  return value;
}

function modelCallSequences(metrics, expectedCalls, location) {
  const calls = new Map();
  metrics.forEach((metric) => {
    const attempts = calls.get(metric.call_sequence) || [];
    attempts.push(metric);
    calls.set(metric.call_sequence, attempts);
  });
  if (calls.size !== expectedCalls) {
    throw new Error(`invalid intent eval report: ${location} model metric call count differs from model_calls`);
  }
  calls.forEach((attempts, callSequence) => {
    attempts.forEach((metric, index) => {
      if (metric.attempt !== index + 1) {
        throw new Error(`invalid intent eval report: ${location} call ${callSequence} attempts are not contiguous`);
      }
      if (index + 1 < attempts.length && metric.outcome === 'succeeded') {
        throw new Error(`invalid intent eval report: ${location} call ${callSequence} retried after success`);
      }
    });
  });
  return calls;
}

function observability(value, location) {
  object(value, location);
  exactKeys(value, [
    'model_calls',
    'tool_calls',
    'distinct_mutation_tools',
    'mutation_tool_calls',
    'clarification_count',
    'validation_failures',
    'simulation_failures',
    'failure_signatures',
    'repeated_errors',
    'repair_attempts',
    'repair_successes',
    'repair_failures',
    'repair_escalations',
    'nudge_count',
    'plan_submissions',
    'plan_acceptances',
    'planned_requirements',
    'plan_compiled_tool_calls',
    'plan_execution_failures',
    'plan_rollbacks',
    'plan_commits',
    'plan_conflicts',
    'intent_route_calls',
    'intent_proposal_acceptances',
    'intent_resolution_acceptances',
    'intent_compile_attempts',
    'intent_compile_successes',
    'intent_commits',
    'intent_rollbacks',
    'intent_conflicts',
    'intent_stale_revision_rejections',
    'intent_extraction_failures',
    'intent_fallback_routes',
    'intent_compiled_operations',
  ], location);
  for (const field of [
    'model_calls',
    'tool_calls',
    'clarification_count',
    'validation_failures',
    'simulation_failures',
    'repeated_errors',
    'repair_attempts',
    'repair_successes',
    'repair_failures',
    'repair_escalations',
    'nudge_count',
    'plan_submissions',
    'plan_acceptances',
    'planned_requirements',
    'plan_compiled_tool_calls',
    'plan_execution_failures',
    'plan_rollbacks',
    'plan_commits',
    'plan_conflicts',
    'intent_route_calls',
    'intent_proposal_acceptances',
    'intent_resolution_acceptances',
    'intent_compile_attempts',
    'intent_compile_successes',
    'intent_commits',
    'intent_rollbacks',
    'intent_conflicts',
    'intent_stale_revision_rejections',
    'intent_extraction_failures',
    'intent_compiled_operations',
  ]) {
    integer(value[field], `${location}.${field}`);
  }
  if (!Array.isArray(value.distinct_mutation_tools)) {
    throw new Error(`invalid intent eval report: ${location}.distinct_mutation_tools must be an array`);
  }
  value.distinct_mutation_tools.forEach((entry, index) => {
    nonEmptyString(entry, `${location}.distinct_mutation_tools[${index}]`);
  });
  sortedUnique(value.distinct_mutation_tools, `${location}.distinct_mutation_tools`);
  for (const field of ['mutation_tool_calls', 'failure_signatures', 'intent_fallback_routes']) {
    object(value[field], `${location}.${field}`);
    for (const [key, count] of Object.entries(value[field])) {
      nonEmptyString(key, `${location}.${field} key`);
      integer(count, `${location}.${field}.${key}`);
    }
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
  exactKeys(report, [
    'schema_version',
    'input_schema_version',
    'mode',
    'intent_protocol_version',
    'intent_adjudicator_version',
    'intent_identity_revision',
    'requested_model',
    'served_model',
    'gateway_id',
    'declared_context_tokens',
    'context_declaration_source',
    'gateway_context_observed_tokens',
    'catalog_identity',
    'provenance',
    'oracle',
    'session_config',
    'outcome',
    'completed',
    'message',
    'question',
    'halt_code',
    'turns',
    'model_call_metrics',
    'draft_revision',
    'ruleset',
    'actual_gates',
    'observability',
    'final_intent',
    'persistence',
    'elapsed_ms',
  ], 'report');
  if (report.schema_version !== 5 || report.input_schema_version !== 3) {
    throw new Error('invalid intent eval report: schema_version must be 5 and input_schema_version must be 3');
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
  observability(report.observability, 'observability');
  object(report.provenance, 'provenance');
  exactKeys(report.provenance, [
    'source_commit',
    'source_dirty',
    'build_source_commit',
    'build_source_dirty',
    'binary_sha256',
    'attestation_kind',
    'run_id',
    'run_order',
    'started_at_unix_ms',
    'ended_at_unix_ms',
  ], 'provenance');
  object(report.oracle, 'oracle');
  exactKeys(report.oracle, ['enabled', 'injected_control_calls'], 'oracle');
  object(report.session_config, 'session_config');
  exactKeys(report.session_config, [
    'max_model_calls',
    'max_tool_calls',
    'max_gate_failures',
    'context_char_budget',
  ], 'session_config');
  object(report.catalog_identity, 'catalog_identity');
  exactKeys(report.catalog_identity, [
    'recipe_id',
    'recipe_version',
    'extractor_revision',
    'normalizer_revision',
    'compiler_revision',
    'simulator_revision',
    'registry_digest',
  ], 'catalog_identity');
  if (report.catalog_identity.recipe_id !== 'starring.private_study_room'
    || report.catalog_identity.recipe_version !== 1
    || report.catalog_identity.extractor_revision !== INTENT_EXTRACTOR_REVISION
    || report.catalog_identity.normalizer_revision !== INTENT_NORMALIZER_REVISION
    || report.catalog_identity.compiler_revision !== 1
    || report.catalog_identity.simulator_revision !== 1
    || report.catalog_identity.registry_digest !== INTENT_REGISTRY_DIGEST) {
    throw new Error('invalid intent eval report: catalog identity is invalid');
  }
  object(report.final_intent, 'final_intent');
  exactKeys(report.final_intent, [
    'status',
    'public_status',
    'receipt',
    'route_decision',
    'binding_fingerprint',
  ], 'final_intent');
  publicStatus(report.final_intent.public_status, 'final_intent.public_status');
  object(report.persistence, 'persistence');
  exactKeys(report.persistence, [
    'backend',
    'store_writes',
    'connection_reopen_count',
    'final_generation',
    'snapshot_schema_version',
    'roundtrip_verified',
  ], 'persistence');
  if (typeof report.final_intent.status !== 'string'
    || typeof report.final_intent.public_status.status !== 'string') {
    throw new Error('invalid intent eval report: final intent status is required');
  }
  if (!/^[0-9a-f]{64}$/.test(report.final_intent.binding_fingerprint)) {
    throw new Error('invalid intent eval report: binding fingerprint must be a SHA-256 hash');
  }
  if (!Object.hasOwn(report.final_intent, 'route_decision')) {
    throw new Error('invalid intent eval report: final_intent.route_decision is required');
  }
  routeDecision(report.final_intent.route_decision, 'final_intent.route_decision');
  if (report.intent_protocol_version !== INTENT_PROTOCOL_VERSION
    || report.intent_adjudicator_version !== INTENT_ADJUDICATOR_VERSION
    || report.intent_identity_revision !== INTENT_IDENTITY_REVISION) {
    throw new Error('invalid intent eval report: V4 intent contract identity is invalid');
  }
  if (report.final_intent.status === 'preview_ready') {
    receiptValue(report.final_intent.receipt, 'final_intent.receipt');
    if (!sameJson(report.final_intent.receipt, report.final_intent.public_status.receipt)) {
      throw new Error('invalid intent eval report: public receipt differs from final receipt');
    }
    const expectedCandidate = candidateIdentityHashes(report);
    if (report.final_intent.receipt.candidate_ruleset_hash
      !== expectedCandidate.candidate_ruleset_hash) {
      throw new Error('invalid intent eval report: candidate_ruleset_hash does not match ruleset');
    }
    if (report.final_intent.receipt.candidate_draft_hash
      !== expectedCandidate.candidate_draft_hash) {
      throw new Error('invalid intent eval report: candidate_draft_hash does not match Draft state');
    }
  } else if (report.final_intent.receipt !== null) {
    throw new Error('invalid intent eval report: non-preview status must not contain a receipt');
  }
  integer(report.draft_revision, 'draft_revision');
  integer(report.elapsed_ms, 'elapsed_ms');
  actualGates(report.actual_gates, 'actual_gates', report.draft_revision);
  if (!Array.isArray(report.model_call_metrics)) {
    throw new Error('invalid intent eval report: model_call_metrics must be an array');
  }
  report.model_call_metrics.forEach((metric, index) => (
    modelCallMetric(metric, `model_call_metrics[${index}]`)
  ));
  integer(report.persistence.store_writes, 'persistence.store_writes');
  integer(report.persistence.connection_reopen_count, 'persistence.connection_reopen_count');
  integer(report.persistence.final_generation, 'persistence.final_generation');
  integer(report.persistence.snapshot_schema_version, 'persistence.snapshot_schema_version');
  if (report.persistence.backend !== 'sqlite_file'
    || report.persistence.snapshot_schema_version !== INTENT_SNAPSHOT_VERSION
    || typeof report.persistence.roundtrip_verified !== 'boolean') {
    throw new Error('invalid intent eval report: SQLite persistence evidence is required');
  }
  for (const [index, turn] of report.turns.entries()) {
    object(turn, `turns[${index}]`);
    exactKeys(turn, [
      'id',
      'input',
      'outcome',
      'completed',
      'message',
      'question',
      'halt_code',
      'last_error',
      'burst_elapsed_ms',
      'elapsed_ms',
      'model_call_metrics',
      'model_calls',
      'model_tool_calls',
      'deterministic_operations',
      'intent_counters',
      'stage_before',
      'stage_after',
      'intent_revision_before',
      'intent_revision_after',
      'route_decision',
      'draft_revision_before',
      'draft_revision_after',
      'draft_changed',
      'actual_gates',
      'restart_after',
      'restart_performed',
    ], `turns[${index}]`);
    object(turn.intent_counters, `turns[${index}].intent_counters`);
    exactKeys(turn.intent_counters, [
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
      'fallback_routes',
    ], `turns[${index}].intent_counters`);
    object(turn.intent_counters.fallback_routes, `turns[${index}].intent_counters.fallback_routes`);
    actualGates(turn.actual_gates, `turns[${index}].actual_gates`, turn.draft_revision_after);
    if (!Object.hasOwn(turn, 'route_decision')) {
      throw new Error(`invalid intent eval report: turns[${index}].route_decision is required`);
    }
    routeDecision(turn.route_decision, `turns[${index}].route_decision`);
    if (typeof turn.id !== 'string'
      || typeof turn.outcome !== 'string'
      || typeof turn.stage_before !== 'string'
      || typeof turn.stage_after !== 'string'
      || typeof turn.draft_changed !== 'boolean'
      || typeof turn.restart_after !== 'boolean'
      || typeof turn.restart_performed !== 'boolean') {
      throw new Error(`invalid intent eval report: turns[${index}] has invalid lifecycle fields`);
    }
    for (const field of [
      'model_calls',
      'model_tool_calls',
      'deterministic_operations',
      'burst_elapsed_ms',
      'elapsed_ms',
    ]) {
      integer(turn[field], `turns[${index}].${field}`);
    }
    if (turn.elapsed_ms < turn.burst_elapsed_ms) {
      throw new Error(`invalid intent eval report: turns[${index}] total duration is below burst duration`);
    }
    if (!Array.isArray(turn.model_call_metrics)) {
      throw new Error(`invalid intent eval report: turns[${index}] model metrics must be an array`);
    }
    turn.model_call_metrics.forEach((metric, metricIndex) => {
      modelCallMetric(metric, `turns[${index}].model_call_metrics[${metricIndex}]`);
    });
    modelCallSequences(turn.model_call_metrics, turn.model_calls, `turns[${index}]`);
    const requestDuration = turn.model_call_metrics
      .reduce((sum, metric) => sum + metric.request_duration_ms, 0);
    if (requestDuration > turn.burst_elapsed_ms + 1) {
      throw new Error(`invalid intent eval report: turns[${index}] request duration exceeds burst duration`);
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
  const flattenedMetrics = report.turns.flatMap((turn) => turn.model_call_metrics);
  if (!sameJson(flattenedMetrics, report.model_call_metrics)) {
    throw new Error('invalid intent eval report: top-level model metrics do not match turn metrics');
  }
  const calls = modelCallSequences(
    report.model_call_metrics,
    report.observability.model_calls,
    'model_call_metrics',
  );
  const sequences = [...calls.keys()];
  if (sequences.some((sequence, index) => sequence !== index + 1)) {
    throw new Error('invalid intent eval report: model call sequences are not contiguous');
  }
  const servedModels = [...new Set(report.model_call_metrics
    .filter((metric) => metric.http_status >= 200
      && metric.http_status < 300
      && metric.served_model !== null)
    .map((metric) => metric.served_model))];
  if (servedModels.length > 1) {
    throw new Error('invalid intent eval report: successful HTTP responses have conflicting model provenance');
  }
  const observedServedModel = servedModels.length === 1 ? servedModels[0] : null;
  if (report.served_model !== observedServedModel) {
    throw new Error('invalid intent eval report: served_model differs from successful HTTP response provenance');
  }
  const turnElapsed = report.turns.reduce((sum, turn) => sum + turn.elapsed_ms, 0);
  if (report.elapsed_ms < turnElapsed) {
    throw new Error('invalid intent eval report: total elapsed time is below summed turn time');
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

function expectedBlockers(expected) {
  if (!Object.hasOwn(expected, 'expectedBlockers')) {
    throw new Error('expectedBlockers is required for intent adjudication assertions');
  }
  const parsed = list(expected.expectedBlockers).map((entry) => {
    const parts = entry.split('|');
    if (parts.length !== 3 || parts[0].length === 0 || parts[1].length === 0) {
      throw new Error(`invalid expectedBlockers entry=${entry}`);
    }
    return [parts[0], parts[1], parts[2] || null];
  });
  return parsed.sort((left, right) => left[0].localeCompare(right[0]));
}

function expectedBoundaries(expected) {
  if (!Object.hasOwn(expected, 'expectedBoundaryViolations')) {
    throw new Error('expectedBoundaryViolations is required for intent adjudication assertions');
  }
  return list(expected.expectedBoundaryViolations).sort();
}

function expectedUnclassifiedRequirements(expected) {
  if (Object.hasOwn(expected, 'expectedUnclassifiedRequirements')) {
    const value = expected.expectedUnclassifiedRequirements;
    if (typeof value === 'string' && value.trim().startsWith('[')) {
      let decoded;
      try {
        decoded = JSON.parse(value);
      } catch {
        throw new Error('expectedUnclassifiedRequirements JSON is invalid');
      }
      if (!Array.isArray(decoded)
        || decoded.some((entry) => typeof entry !== 'string' || entry.trim().length === 0)) {
        throw new Error('expectedUnclassifiedRequirements JSON must be an array of non-empty strings');
      }
      return { exact: decoded.map((entry) => entry.trim()).sort(), contains: [] };
    }
    return { exact: list(value).sort(), contains: [] };
  }
  if (Object.hasOwn(expected, 'expectedUnclassifiedEvidenceContains')) {
    return { exact: null, contains: list(expected.expectedUnclassifiedEvidenceContains).sort() };
  }
  return { exact: [], contains: [] };
}

function routeKind(route) {
  return route === 'resolve_intent_decision' ? 'private_study_room' : route;
}

function blockerProjection(decision) {
  return decision.blockers.map((blocker) => [blocker.id, blocker.status, blocker.policy_id]);
}

function deterministicCapabilityResponse(decision, localeIndex) {
  const labels = decision.blockers.map((blocker) => {
    const label = CAPABILITY_LABELS[blocker.id][localeIndex];
    const status = CAPABILITY_STATUSES[blocker.status][localeIndex];
    const base = `${label} (${status})`;
    if (blocker.id !== 'unclassified_intent_requirement') {
      return base;
    }
    return `${base}: ${blocker.evidence.map((entry) => entry.description).join(', ')}`;
  }).join(', ');
  if (localeIndex === 1) {
    return `요청을 그대로 보존했지만 다음 필수 역량을 현재 제공할 수 없어 컴파일하지 않았습니다: ${labels}. 일부만 만들거나 요구사항을 약화하지 않았습니다.`;
  }
  return `I preserved the request, but did not compile it because these required capabilities are not currently supported: ${labels}. I did not build a partial or weakened version.`;
}

function deterministicRejectResponse(decision, localeIndex) {
  const labels = decision.boundary_violations
    .map((violation) => BOUNDARY_LABELS[violation.id][localeIndex])
    .join(', ');
  if (localeIndex === 1) {
    return `안전한 설계는 도울 수 있지만 다음 요청된 안전 경계는 넘을 수 없습니다: ${labels}. 검증, 미리보기, 사용자 승인, 비밀정보 보호는 계속 적용됩니다.`;
  }
  return `I can help with a safe design, but cannot cross these requested safety boundaries: ${labels}. Validation, preview, user approval, and secret protection remain enforced.`;
}

function deterministicResponses(decision) {
  if (decision.kind === 'typed_planner') {
    return [
      'I routed this supported custom static automation to the typed planner. No live system was changed.',
      '지원되는 정적 커스텀 자동화로 분류해 타입 기반 플래너로 전달했습니다. 라이브 시스템은 변경하지 않았습니다.',
    ];
  }
  if (decision.kind === 'capability_gap') {
    return [
      deterministicCapabilityResponse(decision, 0),
      deterministicCapabilityResponse(decision, 1),
    ];
  }
  if (decision.kind === 'reject') {
    return [deterministicRejectResponse(decision, 0), deterministicRejectResponse(decision, 1)];
  }
  return [];
}

function normalizedDiscussionText(value) {
  return value.trim()
    .replace(/\r\n?/gu, '\n')
    .replace(/\\r\\n|\\n|\\r/gu, '\n');
}

function observableFinishReasons(context) {
  const response = context?.providerResponse;
  const containers = [response, response?.metadata, context?.metadata];
  let raw = response?.raw;
  if (typeof raw === 'string' && raw.trim().startsWith('{')) {
    try {
      raw = JSON.parse(raw);
    } catch {
      raw = null;
    }
  }
  if (raw && typeof raw === 'object') {
    containers.push(raw, raw.choices?.[0]);
  }
  const reasons = [];
  for (const container of containers) {
    if (!container || typeof container !== 'object') {
      continue;
    }
    for (const field of ['finishReason', 'finish_reason', 'finishReasons', 'finish_reasons']) {
      const value = container[field];
      if (Array.isArray(value)) {
        reasons.push(...value);
      } else if (value !== undefined && value !== null) {
        reasons.push(value);
      }
    }
  }
  return [...new Set(reasons
    .filter((value) => typeof value === 'string' && value.trim().length > 0)
    .map((value) => value.trim().toLowerCase().replace(/[\s-]+/gu, '_')))];
}

function balancedDiscussionDelimiters(value) {
  const pairs = { ')': '(', ']': '[', '}': '{' };
  const openings = new Set(Object.values(pairs));
  const stack = [];
  for (const character of value) {
    if (openings.has(character)) {
      stack.push(character);
    } else if (Object.hasOwn(pairs, character) && stack.pop() !== pairs[character]) {
      return false;
    }
  }
  if (stack.length !== 0) {
    return false;
  }
  const characters = [...value];
  let doubleOpenings = 0;
  let doubleClosings = 0;
  let singleOpenings = 0;
  let singleClosings = 0;
  const isWordCharacter = (character) => (
    typeof character === 'string' && /[\p{L}\p{M}\p{N}]/u.test(character)
  );
  for (const [index, character] of characters.entries()) {
    if (character === '“') {
      doubleOpenings += 1;
    } else if (character === '”') {
      doubleClosings += 1;
    } else if (character === '‘') {
      singleOpenings += 1;
    } else if (character === '’') {
      const intraWord = isWordCharacter(characters[index - 1])
        && isWordCharacter(characters[index + 1]);
      const pluralPossessive = singleOpenings === singleClosings
        && /s/iu.test(characters[index - 1] ?? '')
        && !isWordCharacter(characters[index + 1]);
      if (!intraWord && !pluralPossessive) {
        singleClosings += 1;
      }
    }
  }
  if (doubleOpenings !== doubleClosings || singleOpenings !== singleClosings) {
    return false;
  }
  const unescapedDoubleQuotes = [...value].filter((character, index, characters) => (
    character === '"' && characters[index - 1] !== '\\'
  )).length;
  if (unescapedDoubleQuotes % 2 !== 0) {
    return false;
  }
  const fenceCount = value.match(/```/gu)?.length ?? 0;
  const withoutFences = value.replace(/```/gu, '');
  const inlineCodeCount = withoutFences.match(/`/gu)?.length ?? 0;
  const boldCount = value.match(/\*\*/gu)?.length ?? 0;
  return fenceCount % 2 === 0 && inlineCodeCount % 2 === 0 && boldCount % 2 === 0;
}

function discussionSentenceCount(value) {
  const segmenter = new Intl.Segmenter(undefined, { granularity: 'sentence' });
  return [...segmenter.segment(value)]
    .map((entry) => entry.segment.trim())
    .filter(Boolean)
    .length;
}

function completeDiscussionTerminal(value) {
  const closers = new Set([
    '"', "'", ')', ']', '}', '”', '’', '」', '』', '】', '）', '］', '｝', '*', '_', '`',
  ]);
  const characters = [...value];
  while (closers.has(characters.at(-1))) {
    characters.pop();
  }
  const terminal = characters.join('');
  return !terminal.endsWith('...')
    && !terminal.endsWith('…')
    && /[.!?。！？]$/u.test(terminal);
}

function discussionResponseFailures(turn, context) {
  const failures = [];
  if (typeof turn.message !== 'string' || turn.message.trim().length === 0) {
    return [`${turn.id} discussion response is empty`];
  }
  const value = normalizedDiscussionText(turn.message);
  const finishReasons = observableFinishReasons(context);
  const cappedReasons = new Set(['length', 'max_tokens', 'max_output_tokens', 'token_limit']);
  if (finishReasons.some((reason) => cappedReasons.has(reason))) {
    failures.push(`${turn.id} discussion response has a completion-limit finish reason`);
  }
  const completedMetrics = turn.model_call_metrics.filter((metric) => metric.outcome === 'succeeded');
  const finalMetric = completedMetrics[completedMetrics.length - 1];
  const metricFinishReason = typeof finalMetric?.finish_reason === 'string'
    ? finalMetric.finish_reason.trim().toLowerCase().replace(/[\s-]+/gu, '_')
    : null;
  const acceptableFinishReasons = new Set(['stop', 'tool_calls']);
  if (cappedReasons.has(metricFinishReason)) {
    failures.push(`${turn.id} discussion response has a completion-limit finish reason`);
  } else if (!acceptableFinishReasons.has(metricFinishReason)) {
    failures.push(`${turn.id} discussion response lacks an acceptable final finish reason`);
  }
  if (value.length > DISCUSSION_MAX_UTF16_UNITS) {
    failures.push(`${turn.id} discussion response is overly long`);
  }
  const sentenceCount = discussionSentenceCount(value);
  if (sentenceCount < 2) {
    failures.push(`${turn.id} discussion response has fewer than two sentences`);
  } else if (sentenceCount > DISCUSSION_MAX_SENTENCES) {
    failures.push(`${turn.id} discussion response has more than four sentences`);
  }
  const lines = value.split('\n');
  if (lines.some((line) => /^\s{0,3}#{1,6}(?:\s+|$)/u.test(line)
    || /^\s*(?:={3,}|-{3,})\s*$/u.test(line))) {
    failures.push(`${turn.id} discussion response contains a Markdown heading`);
  }
  const tableSeparator = /^\s*\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)+\|?\s*$/u;
  if (lines.some((line) => tableSeparator.test(line))) {
    failures.push(`${turn.id} discussion response contains a Markdown table`);
  }
  const listItems = lines.filter((line) => (
    /^\s{0,3}(?:[-*+]\s+|\d+[.)]\s+)/u.test(line)
  )).length;
  if (listItems > DISCUSSION_MAX_LIST_ITEMS) {
    failures.push(`${turn.id} discussion response contains a long list`);
  }
  if (!completeDiscussionTerminal(value)) {
    failures.push(`${turn.id} discussion response has an obviously unfinished ending`);
  }
  if (!balancedDiscussionDelimiters(value)) {
    failures.push(`${turn.id} discussion response has unbalanced delimiters`);
  }
  return failures;
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
  return [
    'request_evidence_hash',
    'compiler_input_hash',
    'semantic_intent_hash',
    'compiled_plan_hash',
    'candidate_ruleset_hash',
    'candidate_draft_hash',
  ]
    .every((field) => /^[0-9a-f]{64}$/.test(receipt[field]));
}

function intentProvenance(output) {
  return checked(output, (report) => {
    const failures = [];
    if (report.requested_model !== 'gpt-5.6-luna') {
      failures.push(`requested_model=${report.requested_model}`);
    }
    if (report.served_model !== 'gpt-5.6-luna') {
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
    if (report.model_call_metrics.some((metric) => (
      !Number.isSafeInteger(metric.prompt_tokens)
        || !Number.isSafeInteger(metric.completion_tokens)
    ))) {
      failures.push('gateway token usage is missing from model call metrics');
    }
    return result(failures.length === 0, failures.length === 0
      ? 'Luna medium cohort source, binary, and declared context policy are exact and clean'
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

function intentOneCallTurns(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const expectedModelCalls = Object.hasOwn(expected, 'expectedModelCallsPerTurn')
      ? list(expected.expectedModelCallsPerTurn).map(Number)
      : report.turns.map(() => 1);
    const expectedToolCalls = Object.hasOwn(expected, 'expectedToolCallsPerTurn')
      ? list(expected.expectedToolCallsPerTurn).map(Number)
      : report.turns.map(() => 1);
    const failures = [];
    if (expectedModelCalls.length !== report.turns.length
      || expectedToolCalls.length !== report.turns.length
      || expectedModelCalls.some((value) => !Number.isInteger(value) || value < 1 || value > 2)
      || expectedToolCalls.some((value) => !Number.isInteger(value) || value < 1 || value > 2)) {
      failures.push('expected per-turn call paths must contain one bounded value per turn');
    }
    for (const [index, turn] of report.turns.entries()) {
      if (turn.model_calls !== expectedModelCalls[index]
        || turn.model_tool_calls !== expectedToolCalls[index]) {
        failures.push(`${turn.id} calls=${turn.model_calls}/${turn.model_tool_calls} expected=${expectedModelCalls[index]}/${expectedToolCalls[index]}`);
      }
      if (turn.model_call_metrics.length !== turn.model_calls
        || turn.model_call_metrics.some((metric) => (
          metric.attempt !== 1
          || metric.outcome !== 'succeeded'
          || metric.http_status < 200
          || metric.http_status >= 300
          || metric.served_model !== report.served_model
          || metric.finish_reason !== 'tool_calls'
        ))) {
        failures.push(`${turn.id} did not use one successful tool-call completion per logical model call`);
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
    for (const field of [
      'repair_attempts',
      'repair_successes',
      'repair_failures',
      'repair_escalations',
    ]) {
      if (report.observability[field] !== 0) {
        failures.push(`${field}=${report.observability[field]} expected=0`);
      }
    }
    return result(failures.length === 0, failures.length === 0
      ? 'every turn used its exact bounded first-attempt tool-call path without repair'
      : failures.join(', '));
  });
}

function parseExpectedJson(expected, field, kind) {
  if (!Object.hasOwn(expected, field)) {
    return null;
  }
  let value;
  try {
    value = typeof expected[field] === 'string'
      ? JSON.parse(expected[field])
      : expected[field];
  } catch {
    throw new Error(`${field} is not valid JSON`);
  }
  if (kind === 'object' && (value === null || typeof value !== 'object' || Array.isArray(value))) {
    throw new Error(`${field} is not a JSON object`);
  }
  if (kind === 'string-array'
    && (!Array.isArray(value) || value.some((entry) => typeof entry !== 'string'))) {
    throw new Error(`${field} is not a JSON string array`);
  }
  return value;
}

function resolveJsonPointer(root, pointer) {
  if (pointer === '') {
    return { found: true, value: root };
  }
  if (typeof pointer !== 'string' || !pointer.startsWith('/')) {
    throw new Error(`invalid RuleSet JSON Pointer=${pointer}`);
  }
  let current = root;
  for (const encoded of pointer.slice(1).split('/')) {
    const token = encoded.replace(/~1/g, '/').replace(/~0/g, '~');
    if (Array.isArray(current)) {
      if (!/^(?:0|[1-9][0-9]*)$/.test(token) || Number(token) >= current.length) {
        return { found: false, value: undefined };
      }
      current = current[Number(token)];
    } else if (current !== null && typeof current === 'object' && Object.hasOwn(current, token)) {
      current = current[token];
    } else {
      return { found: false, value: undefined };
    }
  }
  return { found: true, value: current };
}

function intentRulesetPathValues(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const values = parseExpectedJson(expected, 'expectedRulesetPathValues', 'object') || {};
    const absent = parseExpectedJson(expected, 'expectedRulesetAbsentPaths', 'string-array') || [];
    const failures = [];
    for (const [pointer, value] of Object.entries(values)) {
      const actual = resolveJsonPointer(report.ruleset, pointer);
      if (!actual.found) {
        failures.push(`missing RuleSet path=${pointer}`);
      } else if (!sameJson(actual.value, value)) {
        failures.push(`RuleSet path=${pointer} value=${JSON.stringify(actual.value)} expected=${JSON.stringify(value)}`);
      }
    }
    for (const pointer of absent) {
      if (resolveJsonPointer(report.ruleset, pointer).found) {
        failures.push(`unexpected RuleSet path=${pointer}`);
      }
    }
    return result(failures.length === 0, failures.length === 0
      ? 'custom copy and untouched recipe defaults match their exact RuleSet paths'
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
      if (turn.outcome !== 'routed') {
        failures.push(`${turn.id} did not reach a deterministic terminal route`);
      }
      if (turn.draft_changed !== false
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

function intentAdjudicationDecision(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const routes = list(expected.expectedRoutePath);
    const expectedBlockerSet = expectedBlockers(expected);
    const expectedBoundarySet = expectedBoundaries(expected);
    const expectedUnclassified = expectedUnclassifiedRequirements(expected);
    const failures = [];
    if (routes.length !== report.turns.length) {
      failures.push(`decision route path length=${report.turns.length} expected=${routes.length}`);
    }
    for (const [index, turn] of report.turns.entries()) {
      const decision = turn.route_decision;
      const expectedKind = routeKind(routes[index]);
      if (!decision) {
        failures.push(`${turn.id} has no deterministic route decision`);
        continue;
      }
      if (expectedKind && decision.kind !== expectedKind) {
        failures.push(`${turn.id} decision=${decision.kind} expected=${expectedKind}`);
      }
      const actualBlockers = blockerProjection(decision);
      if (!sameJson(actualBlockers, expectedBlockerSet)) {
        failures.push(`${turn.id} blockers=${JSON.stringify(actualBlockers)} expected=${JSON.stringify(expectedBlockerSet)}`);
      }
      const actualBoundaries = decision.boundary_violations.map((violation) => violation.id);
      if (!sameJson(actualBoundaries, expectedBoundarySet)) {
        failures.push(`${turn.id} boundaries=${JSON.stringify(actualBoundaries)} expected=${JSON.stringify(expectedBoundarySet)}`);
      }
      const actualUnclassified = [...decision.unclassified_requirements].sort();
      const canonicalInput = turn.input.split(/\s+/u).filter(Boolean).join(' ');
      const grounded = actualUnclassified.every((value) => canonicalInput.includes(value));
      if (!grounded) {
        failures.push(`${turn.id} contains unclassified evidence not grounded in its human input`);
      }
      if (expectedUnclassified.exact !== null
        && !sameJson(actualUnclassified, expectedUnclassified.exact)) {
        failures.push(`${turn.id} unclassified=${JSON.stringify(actualUnclassified)} expected=${JSON.stringify(expectedUnclassified.exact)}`);
      }
      if (expectedUnclassified.exact === null) {
        const containsEvery = expectedUnclassified.contains.every((expectedValue) => (
          actualUnclassified.some((actualValue) => actualValue.includes(expectedValue))
        ));
        if (actualUnclassified.length !== expectedUnclassified.contains.length
          || !containsEvery) {
          failures.push(`${turn.id} unclassified=${JSON.stringify(actualUnclassified)} expected grounded evidence containing=${JSON.stringify(expectedUnclassified.contains)}`);
        }
      }
      if (decision.kind === 'private_study_room') {
        if (!sameJson(decision.route_target, {
          recipe_id: 'starring.private_study_room',
          recipe_version: 1,
        })) {
          failures.push(`${turn.id} did not pin the private StudyRoom recipe`);
        }
      } else if (decision.route_target !== null) {
        failures.push(`${turn.id} non-recipe route has a recipe target`);
      }
      if (['typed_planner', 'capability_gap', 'reject', 'discussion'].includes(decision.kind)) {
        const counters = turn.intent_counters;
        if (turn.outcome !== 'routed'
          || turn.completed !== false
          || turn.question !== null
          || turn.draft_changed !== false
          || turn.draft_revision_before !== turn.draft_revision_after
          || turn.deterministic_operations !== 0
          || counters.compile_attempts !== 0
          || counters.compile_successes !== 0
          || counters.commits !== 0
          || counters.rollbacks !== 0
          || counters.conflicts !== 0) {
          failures.push(`${turn.id} terminal route compiled or mutated canonical state`);
        }
        const responses = deterministicResponses(decision);
        if (decision.kind !== 'discussion' && !responses.includes(turn.message)) {
          failures.push(`${turn.id} surfaced a non-deterministic terminal response`);
        }
        if (decision.kind === 'discussion') {
          failures.push(...discussionResponseFailures(turn, context));
        }
      }
    }
    for (let index = 0; index + 1 < report.turns.length; index += 1) {
      const pending = report.turns[index];
      const resolved = report.turns[index + 1];
      if (pending.stage_after === 'awaiting_decision') {
        if (resolved.stage_before !== 'awaiting_decision'
          || !sameJson(pending.route_decision, resolved.route_decision)) {
          failures.push(`${pending.id} pending route decision changed during resolution`);
        }
      }
    }
    const terminal = report.turns[report.turns.length - 1];
    const decisions = report.turns
      .map((turn) => turn.route_decision)
      .filter((decision) => decision !== null);
    const lastDecision = decisions.length > 0 ? decisions[decisions.length - 1] : null;
    if (!sameJson(report.final_intent.route_decision, lastDecision)) {
      failures.push('final route decision differs from the last reported decision');
    }
    if (report.message !== terminal.message) {
      failures.push('top-level response differs from the terminal turn response');
    }
    return result(failures.length === 0, failures.length === 0
      ? 'deterministic adjudication, exact blockers, and durable decision identity match'
      : failures.join(', '));
  });
}

function normalizerDiscussionTurn(turn, location, failures) {
  const counters = turn.intent_counters;
  const fallbackRoutes = Object.entries(counters.fallback_routes)
    .filter(([, count]) => count > 0);
  if (turn.route_decision?.kind !== 'discussion'
    || turn.outcome !== 'routed'
    || turn.completed !== false
    || turn.stage_before !== 'empty'
    || turn.stage_after !== 'empty'
    || turn.draft_changed !== false
    || turn.draft_revision_before !== turn.draft_revision_after
    || turn.deterministic_operations !== 0
    || counters.route_calls !== 1
    || counters.proposal_acceptances !== 0
    || counters.compile_attempts !== 0
    || counters.compile_successes !== 0
    || counters.commits !== 0
    || fallbackRoutes.length !== 1
    || fallbackRoutes[0][0] !== 'discussion'
    || fallbackRoutes[0][1] !== 1
    || typeof turn.message !== 'string'
    || turn.message.length === 0
    || turn.question !== null) {
    failures.push(`${location} is not an exact mutation-free discussion route`);
  }
}

function normalizerPreviewTurn(turn, location, failures) {
  if (turn.route_decision?.kind !== 'private_study_room'
    || !sameJson(turn.route_decision.route_target, {
      recipe_id: 'starring.private_study_room',
      recipe_version: 1,
    })
    || turn.outcome !== 'ready'
    || turn.completed !== true
    || turn.stage_before !== 'empty'
    || turn.stage_after !== 'preview_ready'
    || turn.draft_changed !== true
    || turn.deterministic_operations !== 22
    || turn.model_calls !== 1
    || turn.model_tool_calls !== 1
    || turn.actual_gates.validation_current !== true
    || turn.actual_gates.simulation_current !== true) {
    failures.push(`${location} is not an exact one-call validated-preview route`);
  }
}

function intentNormalizerBehavior(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const contract = expected.normalizerContract;
    const failures = [];
    const discussionContracts = new Set([
      'same_target_hold',
      'korean_compound_discussion',
      'multi_sentence_metalinguistic_copy',
    ]);
    if (discussionContracts.has(contract)) {
      if (report.turns.length !== 1) {
        failures.push(`discussion contract turn count=${report.turns.length} expected=1`);
      } else {
        normalizerDiscussionTurn(report.turns[0], report.turns[0].id, failures);
      }
      if (report.final_intent.status !== 'empty'
        || report.final_intent.receipt !== null
        || report.persistence.connection_reopen_count !== 0
        || report.persistence.roundtrip_verified !== false) {
        failures.push('discussion contract persisted a preview or unexpected restart');
      }
    } else if (contract === 'validated_preview_disambiguation') {
      if (expected.normalizerBaselineCase !== 'intent_private_study_room_en') {
        failures.push('validated-preview disambiguation lacks its pinned equivalence baseline');
      }
      if (report.turns.length !== 1) {
        failures.push(`validated-preview contract turn count=${report.turns.length} expected=1`);
      } else {
        normalizerPreviewTurn(report.turns[0], report.turns[0].id, failures);
      }
      if (report.final_intent.status !== 'preview_ready'
        || report.final_intent.receipt?.compiled_operations !== 22
        || report.actual_gates.validation_current !== true
        || report.actual_gates.simulation_current !== true) {
        failures.push('validated-preview disambiguation did not produce the default gated recipe');
      }
    } else if (contract === 'discussion_restart_restore') {
      if (report.turns.length !== 2) {
        failures.push(`discussion restart turn count=${report.turns.length} expected=2`);
      } else {
        const [discussion, build] = report.turns;
        normalizerDiscussionTurn(discussion, discussion.id, failures);
        normalizerPreviewTurn(build, build.id, failures);
        if (discussion.restart_after !== true
          || discussion.restart_performed !== true
          || discussion.stage_after !== build.stage_before
          || discussion.intent_revision_after !== build.intent_revision_before
          || discussion.draft_revision_after !== build.draft_revision_before) {
          failures.push('discussion route did not survive an exact restart boundary');
        }
      }
      if (report.persistence.connection_reopen_count !== 1
        || report.persistence.roundtrip_verified !== true
        || report.persistence.store_writes !== 2
        || report.persistence.final_generation !== 2
        || report.final_intent.status !== 'preview_ready') {
        failures.push('discussion restart persistence evidence is incomplete');
      }
    } else {
      failures.push(`unknown normalizer contract=${String(contract)}`);
    }
    return result(failures.length === 0, failures.length === 0
      ? `normalizer contract ${contract} matches the live route`
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
  candidateIdentityHashes,
  intentAdjudicationDecision,
  intentDecisionFlow,
  intentHardLatency,
  intentNoMutationFallback,
  intentNormalizerBehavior,
  intentOneCallTurns,
  intentOracleIsolation,
  intentProvenance,
  intentReceipt,
  intentRestartContinuity,
  intentRulesetPathValues,
  intentRouteStage,
  parseReport,
};
