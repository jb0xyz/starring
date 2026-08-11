const { createHash, randomUUID } = require('node:crypto');
const { execFileSync, spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const {
  assess,
  MINIMUM_RUNS_BY_CASE_ID,
  REQUIRED_CASE_IDS,
  REQUIRED_SAMPLE_TOTAL,
} = require('./acceptance');
const { cargoExecutable } = require('./provider');
const { summarize } = require('./summarize');

const MATRIX_SCHEMA_VERSION = 1;
const MATRIX_PROFILE = 'luna_v4_repeated_acceptance';
const WORKER_IDENTITY = Object.freeze({
  schema_version: 1,
  status: 'ok',
  provider: 'codex_chatgpt',
  model: 'gpt-5.6-luna',
  reasoning_effort: 'medium',
  auth_mode: 'chatgpt',
  codex_cli_version: 'codex-cli 0.147.0-alpha.6.5',
  concurrency_limit: 1,
  queue_capacity: 0,
  request_timeout_ms: 55000,
});
const STATE_FILE = 'state.json';
const OUTPUT_FILES = Object.freeze({
  combined: 'combined.json',
  summary: 'summary.json',
  acceptance: 'acceptance.json',
  failures: 'failures.json',
  manifest: 'manifest.json',
});
const SUPPLEMENTAL_CASE_ORDER = Object.freeze([
  'intent_private_study_room_en',
  'intent_private_study_room_custom_details',
  'intent_private_study_room_custom_copy_only',
  'intent_private_study_room_mutation_hub',
  'intent_private_study_room_ko',
  'intent_private_study_room_mutation_close',
  'intent_private_study_room_mutation_naming',
  'intent_private_study_room_mutation_control',
  'intent_normalizer_same_target_hold',
  'intent_normalizer_korean_compound_discussion',
  'intent_normalizer_multi_sentence_metalinguistic_copy',
  'intent_typed_planner_fallback',
  'intent_creator_only_close_gap',
  'intent_stateful_game_gap',
  'intent_reject_live_mutation',
  'intent_reject_secret_disclosure',
  'intent_reject_skip_approval',
  'intent_reject_all_gate_bypass',
  'intent_redaction_copy_typed_planner',
  'intent_unknown_external_capability_gap',
  'intent_private_study_room_en_paraphrase',
  'intent_normalizer_validated_preview_disambiguation',
  'intent_normalizer_discussion_restart_then_build',
  'intent_private_study_room_missing_hub',
  'intent_private_study_room_restart_pending',
  'intent_discussion_then_build',
]);

class MatrixError extends Error {
  constructor(code, message = code) {
    super(message);
    this.name = 'MatrixError';
    this.code = code;
  }
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

function digest(value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(JSON.stringify(stable(value)));
  return createHash('sha256').update(bytes).digest('hex');
}

function serializedJsonDigest(value) {
  return digest(Buffer.from(`${JSON.stringify(value, null, 2)}\n`));
}

function exactArray(left, right) {
  return left.length === right.length && left.every((entry, index) => entry === right[index]);
}

function parseCaseCatalog(contents) {
  const catalog = [];
  const sections = contents.split(/(?=^- description: )/m).filter((section) => (
    section.startsWith('- description: ')
  ));
  for (const section of sections) {
    const description = section.match(/^- description:\s+([^\n]+)\s*$/m)?.[1]?.trim();
    const caseId = section.match(/^\s+caseId:\s+(\S+)\s*$/m)?.[1];
    const outcomes = section.match(/^\s+expectedOutcomes:\s+([^\n]+)\s*$/m)?.[1]
      ?.split(',')
      .map((value) => value.trim())
      .filter(Boolean);
    const perTurnMatch = section.match(
      /^\s+expectedModelCallsPerTurn:\s+"?(\d+)"?\s*$/m,
    );
    const modelCallsPerTurn = perTurnMatch ? Number(perTurnMatch[1]) : 1;
    if (!description
      || !caseId
      || !Array.isArray(outcomes)
      || outcomes.length === 0
      || !Number.isSafeInteger(modelCallsPerTurn)
      || modelCallsPerTurn < 1) {
      throw new MatrixError('invalid_case_catalog');
    }
    catalog.push({
      case_id: caseId,
      description,
      expected_model_calls_per_sample: outcomes.length * modelCallsPerTurn,
    });
  }
  const ids = catalog.map((entry) => entry.case_id);
  if (!exactArray(ids, REQUIRED_CASE_IDS)
    || new Set(ids).size !== ids.length
    || new Set(catalog.map((entry) => entry.description)).size !== catalog.length) {
    throw new MatrixError('case_catalog_manifest_mismatch');
  }
  return catalog;
}

function escapePattern(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function buildPlan(catalog) {
  const descriptions = new Map(catalog.map((entry) => [entry.case_id, entry.description]));
  const modelCallsByCaseId = Object.fromEntries(catalog.map((entry) => {
    if (!Number.isSafeInteger(entry.expected_model_calls_per_sample)
      || entry.expected_model_calls_per_sample < 1) {
      throw new MatrixError('invalid_expected_model_calls', entry.case_id);
    }
    return [entry.case_id, entry.expected_model_calls_per_sample];
  }));
  const floors = Object.fromEntries(REQUIRED_CASE_IDS.map((caseId) => {
    const floor = MINIMUM_RUNS_BY_CASE_ID[caseId];
    if (!Number.isSafeInteger(floor) || floor < 1) {
      throw new MatrixError('invalid_acceptance_floor', caseId);
    }
    return [caseId, floor];
  }));
  const total = Object.values(floors).reduce((sum, value) => sum + value, 0);
  if (total !== REQUIRED_SAMPLE_TOTAL) {
    throw new MatrixError('acceptance_floor_total_mismatch');
  }
  const phaseSpecs = [{
    id: 'smoke-all-cases',
    case_ids: [...REQUIRED_CASE_IDS],
    repeat: 1,
    filter_pattern: null,
  }];
  if (!exactArray([...SUPPLEMENTAL_CASE_ORDER].sort(), [...REQUIRED_CASE_IDS].sort())) {
    throw new MatrixError('supplemental_case_order_mismatch');
  }
  for (const caseId of SUPPLEMENTAL_CASE_ORDER) {
    const repeat = floors[caseId] - 1;
    if (repeat === 0) {
      continue;
    }
    phaseSpecs.push({
      id: `supplement-${caseId}`,
      case_ids: [caseId],
      repeat,
      filter_pattern: `^${escapePattern(descriptions.get(caseId))}$`,
    });
  }
  let nextOrder = 1;
  const phases = phaseSpecs.map((phase, index) => {
    const expectedRows = phase.case_ids.length * phase.repeat;
    const expectedModelCalls = phase.case_ids.reduce(
      (sum, caseId) => sum + modelCallsByCaseId[caseId] * phase.repeat,
      0,
    );
    const planned = {
      index,
      ...phase,
      expected_rows: expectedRows,
      expected_model_calls: expectedModelCalls,
      first_run_order: nextOrder,
      last_run_order: nextOrder + expectedRows - 1,
    };
    nextOrder += expectedRows;
    return planned;
  });
  if (nextOrder - 1 !== REQUIRED_SAMPLE_TOTAL) {
    throw new MatrixError('matrix_plan_total_mismatch');
  }
  const totalExpectedModelCalls = REQUIRED_CASE_IDS.reduce(
    (sum, caseId) => sum + modelCallsByCaseId[caseId] * floors[caseId],
    0,
  );
  if (phases.reduce((sum, phase) => sum + phase.expected_model_calls, 0)
    !== totalExpectedModelCalls) {
    throw new MatrixError('matrix_model_call_plan_mismatch');
  }
  return {
    schema_version: MATRIX_SCHEMA_VERSION,
    profile: MATRIX_PROFILE,
    case_count: REQUIRED_CASE_IDS.length,
    required_sample_total: REQUIRED_SAMPLE_TOTAL,
    minimum_runs_by_case_id: floors,
    expected_model_calls_by_case_id: modelCallsByCaseId,
    total_expected_model_calls: totalExpectedModelCalls,
    concurrency: 1,
    cache: false,
    share: false,
    promptfoo_write: false,
    phases,
  };
}

function parseArgs(argv) {
  const options = { output: null, resume: false, dryRun: false };
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index];
    if (value === '--output') {
      if (options.output || !argv[index + 1] || argv[index + 1].startsWith('--')) {
        throw new MatrixError('invalid_output_argument');
      }
      options.output = path.resolve(argv[index + 1]);
      index += 1;
    } else if (value === '--resume') {
      options.resume = true;
    } else if (value === '--dry-run') {
      options.dryRun = true;
    } else {
      throw new MatrixError('unknown_argument', `unknown argument ${value}`);
    }
  }
  if (!options.output) {
    throw new MatrixError('output_required');
  }
  if (options.resume && options.dryRun) {
    throw new MatrixError('resume_dry_run_conflict');
  }
  return options;
}

function validateOutputLocation(output, resultsRoot) {
  const resolvedOutput = path.resolve(output);
  const resolvedRoot = path.resolve(resultsRoot);
  const relative = path.relative(resolvedRoot, resolvedOutput);
  if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new MatrixError('output_must_be_under_results');
  }
  return resolvedOutput;
}

function toolingIdentity() {
  const packageDocument = JSON.parse(fs.readFileSync(path.join(__dirname, 'package.json'), 'utf8'));
  const promptfooDocument = JSON.parse(
    fs.readFileSync(path.join(__dirname, 'node_modules', 'promptfoo', 'package.json'), 'utf8'),
  );
  const pinnedPromptfoo = packageDocument.devDependencies?.promptfoo;
  if (typeof pinnedPromptfoo !== 'string' || promptfooDocument.version !== pinnedPromptfoo) {
    throw new MatrixError('promptfoo_version_mismatch');
  }
  if (!/^v(?:2[4-9]|[3-9][0-9])\.[0-9]+\.[0-9]+$/.test(process.version)) {
    throw new MatrixError('unsupported_node_version');
  }
  const cargo = cargoExecutable();
  const promptfooEntrypoint = fs.realpathSync(
    path.join(__dirname, 'node_modules', '.bin', 'promptfoo'),
  );
  return {
    node: process.version,
    node_executable_sha256: digest(fs.readFileSync(process.execPath)),
    promptfoo: promptfooDocument.version,
    promptfoo_entrypoint_sha256: digest(fs.readFileSync(promptfooEntrypoint)),
    promptfoo_package_sha256: digest(
      fs.readFileSync(path.join(__dirname, 'node_modules', 'promptfoo', 'package.json')),
    ),
    package_lock_sha256: digest(fs.readFileSync(path.join(__dirname, 'package-lock.json'))),
    cargo_executable: cargo,
    cargo_executable_sha256: digest(fs.readFileSync(cargo)),
    cargo_version: execFileSync(cargo, ['--version'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    }).trim(),
  };
}

function finalizerIdentity(source) {
  return {
    schema_version: MATRIX_SCHEMA_VERSION,
    source_commit: source.commit,
    matrix_sha256: digest(fs.readFileSync(__filename)),
    acceptance_sha256: digest(fs.readFileSync(path.join(__dirname, 'acceptance.js'))),
    summarize_sha256: digest(fs.readFileSync(path.join(__dirname, 'summarize.js'))),
  };
}

function sourceState(root) {
  const commit = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
  const status = execFileSync('git', ['-C', root, 'status', '--porcelain', '--untracked-files=normal'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
  if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(commit)) {
    throw new MatrixError('invalid_source_commit');
  }
  return { commit, dirty: status.length > 0 };
}

function assertSourceBoundary(expectedCommit, actual) {
  if (actual?.commit !== expectedCommit || actual.dirty !== false) {
    throw new MatrixError('source_boundary_changed');
  }
}

function gatePlan(tooling) {
  return [{
    id: 'design-harness-rust',
    command: tooling.cargo_executable,
    args: ['test', '-p', 'design-harness'],
    cwd: 'root',
    timeout_ms: 600000,
  }, {
    id: 'design-harness-js',
    command: 'npm',
    args: ['test'],
    cwd: 'eval',
    timeout_ms: 120000,
  }];
}

const activeChildren = new Set();

function terminateProcessTree(child, signal) {
  if (!child.pid) {
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {
    }
  }
}

function terminateActiveChildren() {
  for (const child of activeChildren) {
    terminateProcessTree(child, 'SIGTERM');
    const force = setTimeout(() => terminateProcessTree(child, 'SIGKILL'), 2000);
    force.unref();
  }
}

function spawnCaptured({ command, args, cwd, environment, timeoutMs }) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd,
      env: environment,
      stdio: ['ignore', 'pipe', 'pipe'],
      detached: true,
    });
    activeChildren.add(child);
    let stdout = '';
    let stderr = '';
    let timedOut = false;
    let forceKill;
    const timeout = setTimeout(() => {
      timedOut = true;
      terminateProcessTree(child, 'SIGTERM');
      forceKill = setTimeout(() => terminateProcessTree(child, 'SIGKILL'), 2000);
    }, timeoutMs);
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      stdout = boundedAppend(stdout, chunk);
    });
    child.stderr.on('data', (chunk) => {
      stderr = boundedAppend(stderr, chunk);
    });
    child.once('error', (error) => {
      clearTimeout(timeout);
      clearTimeout(forceKill);
      activeChildren.delete(child);
      rejectPromise(error);
    });
    child.once('close', (code, signal) => {
      clearTimeout(timeout);
      clearTimeout(forceKill);
      activeChildren.delete(child);
      resolvePromise({ code, signal, stdout, stderr, timed_out: timedOut });
    });
  });
}

function spawnCommand({ command, args, cwd, environment, timeoutMs }) {
  return spawnCaptured({ command, args, cwd, environment, timeoutMs });
}

function gateEnvironment(environment) {
  const clean = modelEnvironment(environment);
  delete clean.STARRING_CODEX_WORKER_TOKEN;
  delete clean.STARRING_CODEX_WORKER_URL;
  return clean;
}

function modelEnvironment(environment) {
  const allowed = [
    'CARGO',
    'CARGO_HOME',
    'CARGO_TARGET_DIR',
    'HOME',
    'LANG',
    'LC_ALL',
    'LC_CTYPE',
    'LOGNAME',
    'MACOSX_DEPLOYMENT_TARGET',
    'PATH',
    'RUSTC',
    'RUSTFLAGS',
    'RUSTUP_HOME',
    'SDKROOT',
    'SHELL',
    'STARRING_CODEX_WORKER_TOKEN',
    'STARRING_CODEX_WORKER_URL',
    'TERM',
    'TMPDIR',
    'USER',
  ];
  return Object.fromEntries(allowed
    .filter((key) => Object.hasOwn(environment, key))
    .map((key) => [key, environment[key]]));
}

function validateWorkerUrl(value) {
  let worker;
  try {
    worker = new URL(value);
  } catch {
    throw new MatrixError('invalid_worker_url');
  }
  if (worker.protocol !== 'http:'
    || worker.hostname !== '127.0.0.1'
    || worker.port !== '18181'
    || worker.username
    || worker.password
    || worker.pathname !== '/'
    || worker.search
    || worker.hash) {
    throw new MatrixError('worker_must_be_exact_loopback');
  }
  return worker;
}

function requestCounters(value, code = 'invalid_worker_request_counters') {
  const accepted = value?.accepted_requests_total;
  const settled = value?.settled_requests_total;
  if (!Number.isSafeInteger(accepted)
    || accepted < 0
    || !Number.isSafeInteger(settled)
    || settled < 0
    || settled > accepted) {
    throw new MatrixError(code);
  }
  return {
    accepted_requests_total: accepted,
    settled_requests_total: settled,
  };
}

function fixedWorkerIdentity(value) {
  return {
    ...WORKER_IDENTITY,
    instance_id: value.instance_id,
    worker_source_sha256: value.worker_source_sha256,
  };
}

function countersEqual(left, right) {
  return left?.accepted_requests_total === right?.accepted_requests_total
    && left?.settled_requests_total === right?.settled_requests_total;
}

function counterDelta(before, after) {
  const start = requestCounters(before, 'invalid_request_counter_state');
  const end = requestCounters(after, 'invalid_request_counter_state');
  const accepted = end.accepted_requests_total - start.accepted_requests_total;
  const settled = end.settled_requests_total - start.settled_requests_total;
  if (!Number.isSafeInteger(accepted)
    || accepted < 0
    || !Number.isSafeInteger(settled)
    || settled < 0) {
    throw new MatrixError('request_counter_regressed');
  }
  return {
    accepted_requests_total: accepted,
    settled_requests_total: settled,
  };
}

function validateWorkerHealth(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new MatrixError('invalid_worker_health');
  }
  for (const [key, expected] of Object.entries(WORKER_IDENTITY)) {
    if (value[key] !== expected) {
      throw new MatrixError('worker_identity_mismatch', `worker ${key} mismatch`);
    }
  }
  if (typeof value.instance_id !== 'string'
    || value.instance_id.length === 0
    || value.instance_id.length > 128
    || value.instance_id !== value.instance_id.trim()) {
    throw new MatrixError('invalid_worker_instance_id');
  }
  if (typeof value.worker_source_sha256 !== 'string'
    || !/^[0-9a-f]{64}$/.test(value.worker_source_sha256)) {
    throw new MatrixError('invalid_worker_source_sha256');
  }
  if (!Number.isSafeInteger(value.active_requests)
    || value.active_requests < 0
    || !Number.isSafeInteger(value.queued_requests)
    || value.queued_requests < 0) {
    throw new MatrixError('invalid_worker_request_counts');
  }
  const counters = requestCounters(value);
  if (counters.accepted_requests_total - counters.settled_requests_total
    !== value.active_requests + value.queued_requests) {
    throw new MatrixError('worker_request_counter_invariant');
  }
  if (value.active_requests !== 0 || value.queued_requests !== 0) {
    throw new MatrixError('worker_not_idle');
  }
  return {
    ...fixedWorkerIdentity(value),
    active_requests: value.active_requests,
    queued_requests: value.queued_requests,
    ...counters,
  };
}

async function fetchWorkerHealth(environment = process.env) {
  const token = environment.STARRING_CODEX_WORKER_TOKEN;
  const worker = validateWorkerUrl(environment.STARRING_CODEX_WORKER_URL || '');
  if (typeof token !== 'string' || token.length === 0) {
    throw new MatrixError('worker_token_required');
  }
  let response;
  try {
    response = await fetch(new URL('/health', worker), {
      method: 'GET',
      headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(5000),
    });
  } catch {
    throw new MatrixError('worker_health_unreachable');
  }
  if (!response.ok) {
    throw new MatrixError('worker_health_rejected', `worker health returned ${response.status}`);
  }
  let body;
  try {
    body = await response.json();
  } catch {
    throw new MatrixError('invalid_worker_health_json');
  }
  return validateWorkerHealth(body);
}

async function localWorkerSourceSha256() {
  const workerPath = path.resolve(__dirname, '..', '..', 'tools', 'codex-worker', 'worker.mjs');
  const module = await import(pathToFileURL(workerPath).href);
  const value = await module.workerSourceSha256();
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/.test(value)) {
    throw new MatrixError('invalid_local_worker_source_sha256');
  }
  return value;
}

function assertLocalWorkerSource(worker, localSource) {
  if (worker.worker_source_sha256 !== localSource) {
    throw new MatrixError('worker_source_not_local');
  }
}

function assertWorkerBoundary(expected, actual) {
  if (!expected || !actual || !Object.keys(expected).every((key) => expected[key] === actual[key])) {
    throw new MatrixError('worker_boundary_changed');
  }
}

function promptfooArguments(phase, output) {
  const args = [
    'eval',
    '-c',
    path.join(__dirname, 'promptfooconfig.intent.yaml'),
    '-j',
    '1',
    '--no-cache',
    '--no-share',
    '--no-write',
    '--repeat',
    String(phase.repeat),
    '--output',
    output,
    '--no-progress-bar',
    '--no-table',
  ];
  if (phase.filter_pattern) {
    args.push('--filter-pattern', phase.filter_pattern);
  }
  return args;
}

function boundedAppend(current, chunk) {
  const combined = current + chunk;
  return combined.length <= 1048576 ? combined : combined.slice(-1048576);
}

function phaseTimeoutMs(phase) {
  if (!Number.isSafeInteger(phase.expected_model_calls) || phase.expected_model_calls < 1) {
    throw new MatrixError('invalid_phase_timeout_plan');
  }
  const timeout = phase.expected_model_calls * 65000 + 30000;
  if (!Number.isSafeInteger(timeout)) {
    throw new MatrixError('invalid_phase_timeout_plan');
  }
  return timeout;
}

function spawnPromptfoo({ phase, output, environment }) {
  const executable = path.join(__dirname, 'node_modules', '.bin', 'promptfoo');
  const args = promptfooArguments(phase, output);
  return spawnCaptured({
    command: executable,
    args,
    cwd: __dirname,
    environment,
    timeoutMs: phaseTimeoutMs(phase),
  });
}

function rowsFrom(document) {
  if (Array.isArray(document?.results?.results)) {
    return document.results.results;
  }
  return Array.isArray(document?.results) ? document.results : [];
}

function rowVars(row) {
  return row.vars || row.testCase?.vars || {};
}

function rowReport(row) {
  return row.response?.metadata && typeof row.response.metadata === 'object'
    ? row.response.metadata
    : null;
}

function modelCallsFromRows(rows) {
  return rows.reduce((sum, row) => {
    const modelCalls = rowReport(row)?.observability?.model_calls;
    if (!Number.isSafeInteger(modelCalls) || modelCalls < 0) {
      throw new MatrixError('invalid_phase_model_call_count');
    }
    const total = sum + modelCalls;
    if (!Number.isSafeInteger(total)) {
      throw new MatrixError('invalid_phase_model_call_count');
    }
    return total;
  }, 0);
}

function validatePhaseDocument(document, phase, boundary) {
  const rows = rowsFrom(document);
  if (rows.length !== phase.expected_rows) {
    throw new MatrixError('phase_sample_count_mismatch');
  }
  const expectedCases = new Set(phase.case_ids);
  const counts = Object.fromEntries(phase.case_ids.map((caseId) => [caseId, 0]));
  const ordered = [];
  for (const row of rows) {
    const caseId = rowVars(row).caseId;
    const report = rowReport(row);
    const provenance = report?.provenance;
    if (!expectedCases.has(caseId)) {
      throw new MatrixError('phase_case_mismatch');
    }
    if (!provenance
      || provenance.run_id !== boundary.run_id
      || provenance.source_commit !== boundary.source_commit
      || provenance.build_source_commit !== boundary.source_commit
      || provenance.source_dirty !== false
      || provenance.build_source_dirty !== false
      || !Number.isSafeInteger(provenance.run_order)) {
      throw new MatrixError('phase_boundary_mismatch');
    }
    counts[caseId] += 1;
    ordered.push({ order: provenance.run_order, row });
  }
  if (phase.case_ids.some((caseId) => counts[caseId] !== phase.repeat)) {
    throw new MatrixError('phase_case_count_mismatch');
  }
  ordered.sort((left, right) => left.order - right.order);
  if (ordered.some((entry, index) => entry.order !== phase.first_run_order + index)) {
    throw new MatrixError('phase_run_order_mismatch');
  }
  return ordered.map((entry) => entry.row);
}

function validateCombinedRows(rows, plan, boundary) {
  if (rows.length !== plan.required_sample_total) {
    throw new MatrixError('combined_sample_count_mismatch');
  }
  const counts = Object.fromEntries(REQUIRED_CASE_IDS.map((caseId) => [caseId, 0]));
  const ordered = [];
  for (const row of rows) {
    const caseId = rowVars(row).caseId;
    const provenance = rowReport(row)?.provenance;
    if (!Object.hasOwn(counts, caseId)
      || !provenance
      || provenance.run_id !== boundary.run_id
      || provenance.source_commit !== boundary.source_commit
      || provenance.build_source_commit !== boundary.source_commit
      || provenance.source_dirty !== false
      || provenance.build_source_dirty !== false
      || !Number.isSafeInteger(provenance.run_order)) {
      throw new MatrixError('combined_boundary_mismatch');
    }
    counts[caseId] += 1;
    ordered.push({ order: provenance.run_order, row });
  }
  if (REQUIRED_CASE_IDS.some((caseId) => counts[caseId] !== plan.minimum_runs_by_case_id[caseId])) {
    throw new MatrixError('combined_case_count_mismatch');
  }
  ordered.sort((left, right) => left.order - right.order);
  if (ordered.some((entry, index) => entry.order !== index + 1)) {
    throw new MatrixError('combined_run_order_mismatch');
  }
  return ordered.map((entry) => entry.row);
}

function failedAssertions(row) {
  const components = row.gradingResult?.componentResults;
  if (!Array.isArray(components)) {
    return [];
  }
  return components.filter((entry) => entry.pass !== true).map((entry) => {
    const value = entry.assertion?.value;
    return typeof value === 'string' ? value.split(':').at(-1) : 'unknown';
  });
}

function failureDocument(rows) {
  const failures = [];
  for (const row of rows) {
    const report = rowReport(row);
    const assertions = failedAssertions(row);
    if (row.success === true && !row.response?.error && assertions.length === 0) {
      continue;
    }
    const metrics = Array.isArray(report?.model_call_metrics) ? report.model_call_metrics : [];
    failures.push({
      case_id: rowVars(row).caseId || null,
      run_order: report?.provenance?.run_order ?? null,
      promptfoo_success: row.success === true,
      provider_error: typeof row.response?.error === 'string' ? row.response.error : null,
      outcome: report?.outcome ?? null,
      halt_code: report?.halt_code ?? null,
      failed_assertions: assertions,
      elapsed_ms: Number.isFinite(report?.elapsed_ms) ? report.elapsed_ms : null,
      model_calls: report?.observability?.model_calls ?? null,
      tool_calls: report?.observability?.tool_calls ?? null,
      prompt_tokens: metrics.reduce((sum, entry) => sum + Number(entry.prompt_tokens || 0), 0),
      completion_tokens: metrics.reduce(
        (sum, entry) => sum + Number(entry.completion_tokens || 0),
        0,
      ),
    });
  }
  return { schema_version: MATRIX_SCHEMA_VERSION, failures };
}

function observation(rows) {
  const reports = rows.map(rowReport).filter(Boolean);
  const metrics = reports.flatMap((report) => (
    Array.isArray(report.model_call_metrics) ? report.model_call_metrics : []
  ));
  return {
    samples: rows.length,
    promptfoo_passes: rows.filter((row) => row.success === true).length,
    provider_errors: rows.filter((row) => row.response?.error).length,
    model_calls: reports.reduce(
      (sum, report) => sum + Number(report.observability?.model_calls || 0),
      0,
    ),
    tool_calls: reports.reduce(
      (sum, report) => sum + Number(report.observability?.tool_calls || 0),
      0,
    ),
    prompt_tokens: metrics.reduce((sum, entry) => sum + Number(entry.prompt_tokens || 0), 0),
    completion_tokens: metrics.reduce(
      (sum, entry) => sum + Number(entry.completion_tokens || 0),
      0,
    ),
    repair_attempts: reports.reduce(
      (sum, report) => sum + Number(report.observability?.repair_attempts || 0),
      0,
    ),
  };
}

function sanitize(value, secret) {
  const text = String(value || '');
  return secret ? text.split(secret).join('[REDACTED]').slice(-4096) : text.slice(-4096);
}

function timestamp(value) {
  return (value instanceof Date ? value : new Date(value)).toISOString();
}

function relativeArtifact(root, artifact) {
  return path.relative(root, artifact);
}

function absoluteArtifact(root, artifact) {
  const resolved = path.resolve(root, artifact);
  const relative = path.relative(root, resolved);
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new MatrixError('artifact_outside_output');
  }
  return resolved;
}

async function atomicWriteJson(file, value) {
  const temporary = `${file}.${process.pid}.${randomUUID()}.tmp`;
  const handle = await fs.promises.open(temporary, 'wx', 0o600);
  await handle.writeFile(`${JSON.stringify(value, null, 2)}\n`, 'utf8');
  await handle.sync();
  await handle.close();
  await fs.promises.rename(temporary, file);
  await syncDirectory(path.dirname(file));
}

async function syncDirectory(directoryPath) {
  const directory = await fs.promises.open(directoryPath, 'r');
  await directory.sync();
  await directory.close();
}

async function syncFileAndDirectory(file) {
  const handle = await fs.promises.open(file, 'r');
  await handle.sync();
  await handle.close();
  await syncDirectory(path.dirname(file));
}

async function scrubPhaseArtifacts(output, secret) {
  const phaseDirectory = path.join(output, 'phases');
  const entries = await fs.promises.readdir(phaseDirectory, { withFileTypes: true });
  let removed = false;
  let secretDetected = false;
  for (const entry of entries) {
    if (!entry.isFile()) {
      throw new MatrixError('unexpected_phase_artifact_entry');
    }
    const artifact = path.join(phaseDirectory, entry.name);
    let containsSecret = false;
    try {
      await assertSecretAbsent(artifact, secret);
    } catch (error) {
      if (error.code !== 'secret_material_detected') {
        throw error;
      }
      containsSecret = true;
      secretDetected = true;
    }
    if (containsSecret || entry.name.endsWith('.tmp.json')) {
      await fs.promises.rm(artifact, { force: true });
      removed = true;
    }
  }
  if (removed) {
    await syncDirectory(phaseDirectory);
  }
  if (secretDetected) {
    throw new MatrixError('secret_material_detected');
  }
}

async function reconcilePhaseArtifacts(output, state, secret) {
  const phaseDirectory = path.join(output, 'phases');
  const entries = await fs.promises.readdir(phaseDirectory, { withFileTypes: true });
  const referenced = new Set(state.phases
    .map((phase) => phase.artifact)
    .filter(Boolean)
    .map((artifact) => path.resolve(output, artifact)));
  let removed = false;
  let secretDetected = false;
  for (const entry of entries) {
    if (!entry.isFile()) {
      throw new MatrixError('unexpected_phase_artifact_entry');
    }
    const artifact = path.join(phaseDirectory, entry.name);
    const keep = referenced.has(path.resolve(artifact)) && !entry.name.endsWith('.tmp.json');
    if (keep) {
      await fs.promises.chmod(artifact, 0o600);
      await assertSecretAbsent(artifact, secret);
      continue;
    }
    try {
      await assertSecretAbsent(artifact, secret);
    } catch (error) {
      if (error.code !== 'secret_material_detected') {
        throw error;
      }
      secretDetected = true;
    }
    await fs.promises.rm(artifact, { force: true });
    removed = true;
  }
  if (removed) {
    await syncDirectory(phaseDirectory);
  }
  if (secretDetected) {
    throw new MatrixError('secret_material_detected');
  }
}

async function readJson(file) {
  return JSON.parse(await fs.promises.readFile(file, 'utf8'));
}

async function fileDigest(file) {
  return digest(await fs.promises.readFile(file));
}

function processIsAlive(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) {
    return false;
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === 'EPERM';
  }
}

async function acquireMatrixLock(resultsRoot) {
  const lockPath = path.join(resultsRoot, '.luna-v4-matrix.lock');
  const owner = randomUUID();
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      const handle = await fs.promises.open(lockPath, 'wx', 0o600);
      await handle.writeFile(`${JSON.stringify({ pid: process.pid, owner })}\n`);
      await handle.sync();
      await handle.close();
      return async () => {
        let current;
        try {
          current = JSON.parse(await fs.promises.readFile(lockPath, 'utf8'));
        } catch {
          return;
        }
        if (current.owner === owner) {
          await fs.promises.rm(lockPath, { force: true });
        }
      };
    } catch (error) {
      if (error.code !== 'EEXIST') {
        throw error;
      }
      let existing;
      try {
        existing = JSON.parse(await fs.promises.readFile(lockPath, 'utf8'));
      } catch {
        existing = null;
      }
      if (processIsAlive(existing?.pid)) {
        throw new MatrixError('matrix_runner_already_active');
      }
      await fs.promises.rm(lockPath, { force: true });
    }
  }
  throw new MatrixError('matrix_lock_unavailable');
}

async function assertSecretAbsent(file, secret) {
  if (typeof secret !== 'string' || secret.length === 0) {
    throw new MatrixError('worker_token_required');
  }
  const bytes = await fs.promises.readFile(file);
  if (bytes.includes(Buffer.from(secret))) {
    throw new MatrixError('secret_material_detected');
  }
}

function validStateRequestCounters(state, plan) {
  try {
    const initial = requestCounters(
      state?.request_counters?.initial,
      'invalid_request_counter_state',
    );
    const storedLast = requestCounters(
      state?.request_counters?.last_observed,
      'invalid_request_counter_state',
    );
    if (initial.accepted_requests_total !== initial.settled_requests_total
      || storedLast.accepted_requests_total !== storedLast.settled_requests_total) {
      return false;
    }
    let last = initial;
    for (const [index, phaseState] of state.phases.entries()) {
      const expected = plan.phases[index]?.expected_model_calls;
      const actual = phaseState.actual_model_calls;
      const record = phaseState.request_counters;
      if (phaseState.expected_model_calls !== expected
        || (actual !== null && (!Number.isSafeInteger(actual) || actual < 0))
        || !record
        || !Object.hasOwn(record, 'before')
        || !Object.hasOwn(record, 'after')
        || !Object.hasOwn(record, 'delta')) {
        return false;
      }
      if (record.before !== null) {
        const before = requestCounters(record.before, 'invalid_request_counter_state');
        if (!countersEqual(before, last)) {
          return false;
        }
      }
      if ((record.after === null) !== (record.delta === null)) {
        return false;
      }
      if (record.after !== null) {
        if (record.before === null || actual === null) {
          return false;
        }
        const after = requestCounters(record.after, 'invalid_request_counter_state');
        const delta = requestCounters(record.delta, 'invalid_request_counter_state');
        if (!countersEqual(counterDelta(record.before, after), delta)
          || delta.accepted_requests_total !== actual
          || delta.settled_requests_total !== actual) {
          return false;
        }
      }
      if (phaseState.status === 'completed') {
        if (record.before === null || record.after === null || actual === null) {
          return false;
        }
        last = requestCounters(record.after, 'invalid_request_counter_state');
      } else if (phaseState.status === 'pending'
        && (actual !== null
          || record.before !== null
          || record.after !== null
          || record.delta !== null)) {
        return false;
      }
    }
    return countersEqual(last, storedLast);
  } catch {
    return false;
  }
}

function initialState(plan, source, worker, tooling, now, runId) {
  const counters = requestCounters(worker);
  return {
    schema_version: MATRIX_SCHEMA_VERSION,
    profile: MATRIX_PROFILE,
    status: 'running',
    run_id: runId,
    source_commit: source.commit,
    plan_digest: digest(plan),
    worker: fixedWorkerIdentity(worker),
    request_counters: {
      initial: { ...counters },
      last_observed: { ...counters },
    },
    tooling,
    created_at: timestamp(now),
    updated_at: timestamp(now),
    completed_at: null,
    plan,
    gates: gatePlan(tooling).map((gate) => ({
      id: gate.id,
      status: 'pending',
      attempts: 0,
      command: gate.command,
      args: [...gate.args],
      cwd: gate.cwd,
      timeout_ms: gate.timeout_ms,
      started_at: null,
      completed_at: null,
      stdout_sha256: null,
      stderr_sha256: null,
      error_detail: null,
    })),
    phases: plan.phases.map((phase) => ({
      id: phase.id,
      expected_model_calls: phase.expected_model_calls,
      actual_model_calls: null,
      request_counters: {
        before: null,
        after: null,
        delta: null,
      },
      status: 'pending',
      attempts: 0,
      artifact: null,
      artifact_sha256: null,
      promptfoo_exit_code: null,
      error_code: null,
      error_detail: null,
    })),
    artifacts: null,
    observed: null,
    finalizer: null,
    failure_evidence: null,
  };
}

function validatedFailureEvidence(value) {
  if (!value
    || value.schema_version !== MATRIX_SCHEMA_VERSION
    || value.path !== OUTPUT_FILES.failures
    || !/^[0-9a-f]{64}$/.test(value.sha256)
    || !value.document
    || typeof value.document !== 'object'
    || !value.document.matrix_error
    || value.sha256 !== serializedJsonDigest(value.document)) {
    throw new MatrixError('deferred_finalization_failure_evidence_missing');
  }
  return value;
}

function validateResumeState(state, plan, source, worker, tooling) {
  const expectedGates = gatePlan(tooling);
  let incompleteSeen = false;
  const phaseStatusesValid = Array.isArray(state?.phases) && state.phases.every((phase) => {
    if (!['pending', 'running', 'failed', 'completed'].includes(phase.status)) {
      return false;
    }
    if (phase.status !== 'completed') {
      incompleteSeen = true;
      return true;
    }
    return !incompleteSeen;
  });
  const allPhasesCompleted = Array.isArray(state?.phases)
    && state.phases.every((phase) => phase.status === 'completed');
  const allGatesPassed = Array.isArray(state?.gates)
    && state.gates.every((gate) => gate.status === 'passed');
  const alreadyFinalized = state?.completed_at !== null
    || state?.artifacts !== null
    || state?.observed !== null
    || (state?.finalizer !== null && state?.finalizer !== undefined);
  if (allPhasesCompleted && alreadyFinalized) {
    throw new MatrixError('cohort_already_finalized');
  }
  const finalizationOnly = state?.source_commit !== source.commit
    && allPhasesCompleted
    && allGatesPassed;
  if (finalizationOnly) {
    validatedFailureEvidence(state.failure_evidence);
  }
  if (state?.schema_version !== MATRIX_SCHEMA_VERSION
    || state.profile !== MATRIX_PROFILE
    || (state.source_commit !== source.commit && !finalizationOnly)
    || state.plan_digest !== digest(plan)
    || digest(state.plan) !== state.plan_digest
    || !state.tooling
    || digest(state.tooling) !== digest(tooling)
    || !state.worker
    || state.worker.instance_id !== worker.instance_id
    || !phaseStatusesValid
    || state.phases.length !== plan.phases.length
    || state.phases.some((phase, index) => phase.id !== plan.phases[index].id)
    || !validStateRequestCounters(state, plan)
    || !Array.isArray(state.gates)
    || state.gates.length !== expectedGates.length
    || state.gates.some((gate, index) => (
      gate.id !== expectedGates[index].id
        || gate.command !== expectedGates[index].command
        || gate.cwd !== expectedGates[index].cwd
        || gate.timeout_ms !== expectedGates[index].timeout_ms
        || !exactArray(gate.args, expectedGates[index].args)
        || !Number.isSafeInteger(gate.attempts)
        || gate.attempts < 0
        || !['pending', 'running', 'failed', 'passed'].includes(gate.status)
    ))
    || typeof state.run_id !== 'string'
    || state.run_id.length === 0) {
    throw new MatrixError('resume_boundary_mismatch');
  }
  assertWorkerBoundary(state.worker, worker);
  if (!countersEqual(state.request_counters.last_observed, requestCounters(worker))) {
    throw new MatrixError('resume_request_counter_mismatch');
  }
  return { finalization_only: finalizationOnly };
}

async function validateCompletedPhases(state, output, boundary, secret) {
  const rows = [];
  for (const [index, phaseState] of state.phases.entries()) {
    if (phaseState.status !== 'completed') {
      continue;
    }
    if (!phaseState.artifact || !phaseState.artifact_sha256) {
      throw new MatrixError('completed_phase_artifact_missing');
    }
    const artifact = absoluteArtifact(output, phaseState.artifact);
    await assertSecretAbsent(artifact, secret);
    if (await fileDigest(artifact) !== phaseState.artifact_sha256) {
      throw new MatrixError('completed_phase_artifact_changed');
    }
    const document = await readJson(artifact);
    const phaseRows = validatePhaseDocument(document, state.plan.phases[index], boundary);
    if (modelCallsFromRows(phaseRows) !== phaseState.actual_model_calls) {
      throw new MatrixError('phase_model_call_count_mismatch');
    }
    rows.push(...phaseRows);
  }
  return rows;
}

async function executeGates({
  state,
  statePath,
  root,
  environment,
  dependencies,
}) {
  for (const gateState of state.gates) {
    if (gateState.status === 'passed') {
      continue;
    }
    assertSourceBoundary(state.source_commit, dependencies.sourceState(root));
    gateState.status = 'running';
    gateState.attempts += 1;
    gateState.started_at = timestamp(dependencies.now());
    gateState.completed_at = null;
    gateState.stdout_sha256 = null;
    gateState.stderr_sha256 = null;
    gateState.error_detail = null;
    state.updated_at = gateState.started_at;
    await atomicWriteJson(statePath, state);
    dependencies.writeOutput(`${JSON.stringify({
      event: 'gate_started',
      gate_id: gateState.id,
    })}\n`);
    let result;
    try {
      const commandEnvironment = gateEnvironment(environment);
      if (gateState.id === 'design-harness-rust') {
        commandEnvironment.CARGO = gateState.command;
        commandEnvironment.PATH = [
          path.dirname(gateState.command),
          commandEnvironment.PATH,
        ].filter(Boolean).join(path.delimiter);
      }
      result = await dependencies.runGate({
        command: gateState.command,
        args: [...gateState.args],
        cwd: gateState.cwd === 'root' ? root : __dirname,
        environment: commandEnvironment,
        timeoutMs: gateState.timeout_ms,
      });
    } catch (error) {
      throw new MatrixError('deterministic_gate_spawn_failed', error.message);
    }
    gateState.completed_at = timestamp(dependencies.now());
    gateState.stdout_sha256 = digest(Buffer.from(result.stdout || ''));
    gateState.stderr_sha256 = digest(Buffer.from(result.stderr || ''));
    if (result.timed_out) {
      gateState.status = 'failed';
      gateState.error_detail = 'gate timed out';
      state.updated_at = gateState.completed_at;
      await atomicWriteJson(statePath, state);
      throw new MatrixError('deterministic_gate_timeout', gateState.id);
    }
    if (result.code !== 0) {
      gateState.status = 'failed';
      gateState.error_detail = sanitize(
        result.stderr || result.stdout || result.signal || result.code,
        environment.STARRING_CODEX_WORKER_TOKEN,
      );
      state.updated_at = gateState.completed_at;
      await atomicWriteJson(statePath, state);
      throw new MatrixError('deterministic_gate_failed', gateState.id);
    }
    assertSourceBoundary(state.source_commit, dependencies.sourceState(root));
    gateState.status = 'passed';
    state.updated_at = gateState.completed_at;
    await atomicWriteJson(statePath, state);
    dependencies.writeOutput(`${JSON.stringify({
      event: 'gate_completed',
      gate_id: gateState.id,
    })}\n`);
  }
}

async function executePendingPhase({
  state,
  phase,
  phaseState,
  output,
  environment,
  dependencies,
  statePath,
  root,
}) {
  const now = dependencies.now();
  phaseState.status = 'running';
  phaseState.attempts += 1;
  phaseState.error_code = null;
  phaseState.error_detail = null;
  phaseState.promptfoo_exit_code = null;
  phaseState.actual_model_calls = null;
  state.status = 'running';
  state.updated_at = timestamp(now);
  await atomicWriteJson(statePath, state);
  dependencies.writeOutput(`${JSON.stringify({
    event: 'phase_started',
    phase_index: phase.index + 1,
    phase_count: state.plan.phases.length,
    phase_id: phase.id,
    attempt: phaseState.attempts,
    expected_rows: phase.expected_rows,
    expected_model_calls: phase.expected_model_calls,
  })}\n`);
  assertSourceBoundary(state.source_commit, dependencies.sourceState(root));
  const preflight = await dependencies.health(environment);
  assertWorkerBoundary(state.worker, preflight);
  const preflightCounters = requestCounters(preflight);
  if (!countersEqual(state.request_counters.last_observed, preflightCounters)) {
    throw new MatrixError('request_counter_discontinuity');
  }
  phaseState.request_counters = {
    before: { ...preflightCounters },
    after: null,
    delta: null,
  };
  state.updated_at = timestamp(dependencies.now());
  await atomicWriteJson(statePath, state);
  const attempt = String(phaseState.attempts).padStart(2, '0');
  const phaseDir = path.join(output, 'phases');
  const temporary = path.join(phaseDir, `${phase.index}-${phase.id}.attempt-${attempt}.tmp.json`);
  const artifact = path.join(phaseDir, `${phase.index}-${phase.id}.attempt-${attempt}.json`);
  await fs.promises.rm(temporary, { force: true });
  const phaseEnvironment = {
    ...modelEnvironment(environment),
    PROMPTFOO_DISABLE_TELEMETRY: '1',
    PROMPTFOO_FAILED_TEST_EXIT_CODE: '100',
    PROMPTFOO_PASS_RATE_THRESHOLD: '100',
    STARRING_EVAL_RUN_ID: state.run_id,
    STARRING_EVAL_RUN_ORDER_OFFSET: String(phase.first_run_order - 1),
  };
  let result;
  try {
    result = await dependencies.spawnPhase({
      phase,
      output: temporary,
      environment: phaseEnvironment,
      args: promptfooArguments(phase, temporary),
    });
  } catch (error) {
    throw new MatrixError('promptfoo_spawn_failed', error.message);
  }
  if (await fs.promises.stat(temporary).then(() => true, () => false)) {
    await fs.promises.rename(temporary, artifact);
    await fs.promises.chmod(artifact, 0o600);
    await syncFileAndDirectory(artifact);
    try {
      await assertSecretAbsent(artifact, environment.STARRING_CODEX_WORKER_TOKEN);
      phaseState.artifact = relativeArtifact(output, artifact);
      phaseState.artifact_sha256 = await fileDigest(artifact);
    } catch (error) {
      await fs.promises.rm(artifact, { force: true });
      throw error;
    }
  }
  phaseState.promptfoo_exit_code = result.code;
  if (!phaseState.artifact) {
    throw new MatrixError(
      result.code === 0 ? 'promptfoo_phase_output_missing' : 'promptfoo_phase_failed',
      sanitize(
        result.stderr || result.stdout || result.signal || result.code,
        environment.STARRING_CODEX_WORKER_TOKEN,
      ),
    );
  }
  if (result.timed_out) {
    throw new MatrixError('promptfoo_phase_timeout');
  }
  const document = await readJson(artifact);
  const rows = validatePhaseDocument(document, phase, {
    run_id: state.run_id,
    source_commit: state.source_commit,
  });
  const actualModelCalls = modelCallsFromRows(rows);
  phaseState.actual_model_calls = actualModelCalls;
  const postflight = await dependencies.health(environment);
  assertWorkerBoundary(state.worker, postflight);
  assertSourceBoundary(state.source_commit, dependencies.sourceState(root));
  const postflightCounters = requestCounters(postflight);
  const delta = counterDelta(preflightCounters, postflightCounters);
  phaseState.request_counters.after = { ...postflightCounters };
  phaseState.request_counters.delta = { ...delta };
  state.updated_at = timestamp(dependencies.now());
  await atomicWriteJson(statePath, state);
  if (delta.accepted_requests_total !== actualModelCalls
    || delta.settled_requests_total !== actualModelCalls) {
    throw new MatrixError('request_counter_delta_mismatch');
  }
  if (![0, 100].includes(result.code)) {
    throw new MatrixError(
      'promptfoo_phase_failed',
      sanitize(
        result.stderr || result.stdout || result.signal || result.code,
        environment.STARRING_CODEX_WORKER_TOKEN,
      ),
    );
  }
  state.request_counters.last_observed = { ...postflightCounters };
  phaseState.status = 'completed';
  phaseState.error_code = null;
  phaseState.error_detail = null;
  state.updated_at = timestamp(dependencies.now());
  await atomicWriteJson(statePath, state);
  dependencies.writeOutput(`${JSON.stringify({
    event: 'phase_completed',
    phase_index: phase.index + 1,
    phase_count: state.plan.phases.length,
    phase_id: phase.id,
    rows: rows.length,
    model_calls: actualModelCalls,
  })}\n`);
  return rows;
}

async function writeFinalArtifacts(
  state,
  output,
  rows,
  dependencies,
  secret,
  finalizer,
  recovery,
) {
  const combined = {
    schema_version: MATRIX_SCHEMA_VERSION,
    profile: MATRIX_PROFILE,
    results: { results: rows },
  };
  const summary = dependencies.summarize(combined);
  const acceptance = dependencies.assess(combined);
  const failures = failureDocument(rows);
  const observed = observation(rows);
  const totalCounterDelta = counterDelta(
    state.request_counters.initial,
    state.request_counters.last_observed,
  );
  const requestCounterClean = totalCounterDelta.accepted_requests_total === observed.model_calls
    && totalCounterDelta.settled_requests_total === observed.model_calls;
  const modelCallPlanMet = observed.model_calls === state.plan.total_expected_model_calls;
  observed.request_counter_clean = requestCounterClean;
  observed.model_call_plan_met = modelCallPlanMet;
  observed.expected_model_calls = state.plan.total_expected_model_calls;
  observed.accepted_requests_delta = totalCounterDelta.accepted_requests_total;
  observed.settled_requests_delta = totalCounterDelta.settled_requests_total;
  const retryFree = state.phases.every((phase) => phase.attempts === 1);
  const gatesPassed = state.gates.every((gate) => gate.status === 'passed');
  const gateRetryFree = state.gates.every((gate) => gate.attempts === 1);
  const promptfooCleanExit = state.phases.every((phase) => phase.promptfoo_exit_code === 0);
  const certificationFailures = [];
  if (!retryFree) {
    certificationFailures.push('phase_retry_detected');
  }
  if (!gatesPassed) {
    certificationFailures.push('deterministic_gate_not_passed');
  }
  if (!gateRetryFree) {
    certificationFailures.push('deterministic_gate_retry_detected');
  }
  if (!promptfooCleanExit) {
    certificationFailures.push('promptfoo_nonzero_exit');
  }
  if (!requestCounterClean) {
    certificationFailures.push('request_counter_mismatch');
  }
  if (!modelCallPlanMet) {
    certificationFailures.push('model_call_plan_mismatch');
  }
  if (acceptance.pass !== true) {
    certificationFailures.push('acceptance_checks_failed');
  }
  const status = certificationFailures.length === 0 ? 'passed' : 'failed';
  const effectiveFinalizer = { ...finalizer };
  let recoveryFile = null;
  if (recovery) {
    const recoveryDirectory = path.join(output, 'recovery');
    await fs.promises.mkdir(recoveryDirectory, { recursive: true, mode: 0o700 });
    await fs.promises.chmod(recoveryDirectory, 0o700);
    recoveryFile = path.join(recoveryDirectory, 'prior-finalization-failure.json');
    const recoveryExists = await fs.promises.stat(recoveryFile).then(() => true, () => false);
    if (recoveryExists) {
      const existing = await readJson(recoveryFile);
      if (serializedJsonDigest(existing) !== serializedJsonDigest(recovery)) {
        throw new MatrixError('deferred_finalization_recovery_changed');
      }
    } else {
      await atomicWriteJson(recoveryFile, recovery);
    }
    await fs.promises.chmod(recoveryFile, 0o600);
    await assertSecretAbsent(recoveryFile, secret);
    effectiveFinalizer.recovery_input = {
      path: relativeArtifact(output, recoveryFile),
      sha256: await fileDigest(recoveryFile),
      original_path: recovery.original_path,
      original_sha256: recovery.original_sha256,
      matrix_error: recovery.document.matrix_error,
    };
  }
  const acceptanceArtifact = {
    ...acceptance,
    evidence_source_commit: state.source_commit,
    finalizer: effectiveFinalizer,
    model_acceptance_pass: acceptance.pass === true,
    pass: status === 'passed',
    status,
    certification_failures: certificationFailures,
  };
  failures.certification_failures = certificationFailures;
  const files = {
    combined: path.join(output, OUTPUT_FILES.combined),
    summary: path.join(output, OUTPUT_FILES.summary),
    acceptance: path.join(output, OUTPUT_FILES.acceptance),
    failures: path.join(output, OUTPUT_FILES.failures),
  };
  if (recoveryFile) {
    files.recovery_input = recoveryFile;
  }
  await atomicWriteJson(files.combined, combined);
  await atomicWriteJson(files.summary, summary);
  await atomicWriteJson(files.acceptance, acceptanceArtifact);
  await atomicWriteJson(files.failures, failures);
  const artifacts = {};
  for (const [name, file] of Object.entries(files)) {
    await assertSecretAbsent(file, secret);
    artifacts[name] = {
      path: relativeArtifact(output, file),
      sha256: await fileDigest(file),
    };
  }
  observed.retry_free = retryFree;
  observed.deterministic_gates_passed = gatesPassed;
  observed.deterministic_gates_retry_free = gateRetryFree;
  observed.promptfoo_clean_exit = promptfooCleanExit;
  const completedAt = timestamp(dependencies.now());
  const manifest = {
    schema_version: MATRIX_SCHEMA_VERSION,
    profile: MATRIX_PROFILE,
    status,
    run_id: state.run_id,
    source_commit: state.source_commit,
    finalizer: effectiveFinalizer,
    plan_digest: state.plan_digest,
    worker: state.worker,
    tooling: state.tooling,
    created_at: state.created_at,
    completed_at: completedAt,
    plan: state.plan,
    gates: state.gates,
    phases: state.phases,
    request_counters: state.request_counters,
    artifacts,
    observed,
  };
  const manifestPath = path.join(output, OUTPUT_FILES.manifest);
  await atomicWriteJson(manifestPath, manifest);
  await assertSecretAbsent(manifestPath, secret);
  artifacts.manifest = {
    path: relativeArtifact(output, manifestPath),
    sha256: await fileDigest(manifestPath),
  };
  state.status = status;
  state.updated_at = completedAt;
  state.completed_at = completedAt;
  state.artifacts = artifacts;
  state.observed = observed;
  state.finalizer = effectiveFinalizer;
  return { acceptance: acceptanceArtifact, failures, manifest, status };
}

function defaultDependencies() {
  return {
    assess,
    summarize,
    sourceState,
    health: fetchWorkerHealth,
    workerSource: localWorkerSourceSha256,
    spawnPhase: spawnPromptfoo,
    runGate: spawnCommand,
    tooling: toolingIdentity,
    resultsRoot: path.join(__dirname, 'results'),
    acquireLock: acquireMatrixLock,
    now: () => new Date(),
    runId: () => `luna-v4-${randomUUID()}`,
    finalizerIdentity,
    beforeFinalStateWrite: async () => {},
    beforeFailureArtifactWrite: async () => {},
    writeOutput: (value) => process.stdout.write(value),
  };
}

async function runMatrix(options, overrides = {}) {
  const dependencies = { ...defaultDependencies(), ...overrides };
  const root = path.resolve(__dirname, '..', '..');
  const environment = { ...process.env, ...(overrides.environment || {}) };
  const output = validateOutputLocation(options.output, dependencies.resultsRoot);
  const statePath = path.join(output, STATE_FILE);
  const releaseLock = options.dryRun
    ? async () => {}
    : await dependencies.acquireLock(dependencies.resultsRoot);
  let state;
  let executionStarted = false;
  let finalizationOnly = false;
  try {
    if (options.resume) {
      await fs.promises.chmod(output, 0o700);
      await fs.promises.chmod(path.join(output, 'phases'), 0o700);
      await scrubPhaseArtifacts(
        output,
        environment.STARRING_CODEX_WORKER_TOKEN,
      );
    }
    const catalog = parseCaseCatalog(
      await fs.promises.readFile(path.join(__dirname, 'intent-cases.yaml'), 'utf8'),
    );
    const plan = buildPlan(catalog);
    const tooling = dependencies.tooling();
    const source = dependencies.sourceState(root);
    if (source.dirty) {
      throw new MatrixError('clean_committed_source_required');
    }
    if (options.dryRun) {
      dependencies.writeOutput(`${JSON.stringify({
        source_commit: source.commit,
        tooling,
        gates: gatePlan(tooling),
        plan,
      }, null, 2)}\n`);
      return { status: 'dry_run', plan };
    }
    const worker = await dependencies.health(environment);
    assertLocalWorkerSource(worker, await dependencies.workerSource());
    if (options.resume) {
      state = await readJson(statePath).catch(() => {
        throw new MatrixError('resume_state_missing');
      });
      const resume = validateResumeState(state, plan, source, worker, tooling);
      finalizationOnly = resume.finalization_only;
    } else {
      await fs.promises.mkdir(output, { recursive: false, mode: 0o700 }).catch((error) => {
        if (error.code === 'EEXIST') {
          throw new MatrixError('output_already_exists');
        }
        throw error;
      });
      await fs.promises.mkdir(path.join(output, 'phases'), { mode: 0o700 });
      state = initialState(
        plan,
        source,
        worker,
        tooling,
        dependencies.now(),
        dependencies.runId(),
      );
      await atomicWriteJson(statePath, state);
    }
    await fs.promises.chmod(output, 0o700);
    await fs.promises.chmod(path.join(output, 'phases'), 0o700);
    await reconcilePhaseArtifacts(
      output,
      state,
      environment.STARRING_CODEX_WORKER_TOKEN,
    );
    const boundary = { run_id: state.run_id, source_commit: state.source_commit };
    executionStarted = true;
    await validateCompletedPhases(
      state,
      output,
      boundary,
      environment.STARRING_CODEX_WORKER_TOKEN,
    );
    await executeGates({
      state,
      statePath,
      root,
      environment,
      dependencies,
    });
    for (const [index, phase] of plan.phases.entries()) {
      const phaseState = state.phases[index];
      if (phaseState.status === 'completed') {
        dependencies.writeOutput(`${JSON.stringify({
          event: 'phase_reused',
          phase_index: phase.index + 1,
          phase_count: plan.phases.length,
          phase_id: phase.id,
          rows: phase.expected_rows,
        })}\n`);
        continue;
      }
      await executePendingPhase({
        state,
        phase,
        phaseState,
        output,
        environment,
        dependencies,
        statePath,
        root,
      });
    }
    const phaseRows = await validateCompletedPhases(
      state,
      output,
      boundary,
      environment.STARRING_CODEX_WORKER_TOKEN,
    );
    const rows = validateCombinedRows(phaseRows, plan, boundary);
    const postflight = await dependencies.health(environment);
    assertWorkerBoundary(state.worker, postflight);
    if (!countersEqual(state.request_counters.last_observed, requestCounters(postflight))) {
      throw new MatrixError('request_counter_discontinuity');
    }
    const finalizerSource = dependencies.sourceState(root);
    assertSourceBoundary(
      finalizationOnly ? source.commit : state.source_commit,
      finalizerSource,
    );
    const finalizer = {
      ...dependencies.finalizerIdentity(finalizerSource),
      mode: finalizationOnly ? 'deferred_completed_phase_artifacts' : 'inline_run',
      ...(finalizationOnly ? { model_requests_executed: 0 } : {}),
    };
    let recovery = null;
    if (finalizationOnly) {
      const failureEvidence = validatedFailureEvidence(state.failure_evidence);
      recovery = {
        schema_version: MATRIX_SCHEMA_VERSION,
        original_path: failureEvidence.path,
        original_sha256: failureEvidence.sha256,
        document: failureEvidence.document,
      };
    }
    const final = await writeFinalArtifacts(
      state,
      output,
      rows,
      dependencies,
      environment.STARRING_CODEX_WORKER_TOKEN,
      finalizer,
      recovery,
    );
    await dependencies.beforeFinalStateWrite({ state, final, output });
    await atomicWriteJson(statePath, state);
    dependencies.writeOutput(`${JSON.stringify({
      status: final.status,
      output,
      observed: state.observed,
      failed_checks: final.acceptance.checks?.filter((entry) => !entry.pass).map((entry) => entry.name) || [],
      failure_rows: final.failures.failures.length,
    }, null, 2)}\n`);
    if (final.status !== 'passed') {
      throw new MatrixError('acceptance_matrix_failed');
    }
    return final;
  } catch (error) {
    if (!executionStarted) {
      throw error;
    }
    const active = state.phases.find((phase) => phase.status === 'running');
    if (active) {
      active.status = 'failed';
      active.error_code = error.code || 'matrix_error';
      active.error_detail = sanitize(error.message, environment.STARRING_CODEX_WORKER_TOKEN);
    }
    let failures = [];
    if (active?.artifact) {
      try {
        failures = failureDocument(rowsFrom(await readJson(
          absoluteArtifact(output, active.artifact),
        ))).failures;
      } catch {
        failures = [];
      }
    }
    if (!finalizationOnly && state.completed_at === null && state.artifacts === null) {
      const failurePath = path.join(output, OUTPUT_FILES.failures);
      const failureArtifact = {
        schema_version: MATRIX_SCHEMA_VERSION,
        matrix_error: {
          code: error.code || 'matrix_error',
          detail: sanitize(error.message, environment.STARRING_CODEX_WORKER_TOKEN),
          phase_id: active?.id || null,
        },
        failures,
      };
      state.failure_evidence = {
        schema_version: MATRIX_SCHEMA_VERSION,
        path: OUTPUT_FILES.failures,
        sha256: serializedJsonDigest(failureArtifact),
        document: failureArtifact,
      };
      state.status = 'failed';
      state.updated_at = timestamp(dependencies.now());
      await atomicWriteJson(statePath, state);
      await dependencies.beforeFailureArtifactWrite({ state, failureArtifact, output });
      await atomicWriteJson(failurePath, failureArtifact);
    }
    if (state.status !== 'passed') {
      state.status = 'failed';
      state.updated_at = timestamp(dependencies.now());
      await atomicWriteJson(statePath, state);
    }
    throw error;
  } finally {
    await releaseLock();
  }
}

if (require.main === module) {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.code || 'matrix_error'}: ${error.message}\n`);
    process.exit(2);
  }
  let interruptedSignal = null;
  const interrupt = (signal) => {
    interruptedSignal = signal;
    terminateActiveChildren();
  };
  const interruptSigint = () => interrupt('SIGINT');
  const interruptSigterm = () => interrupt('SIGTERM');
  process.once('SIGINT', interruptSigint);
  process.once('SIGTERM', interruptSigterm);
  runMatrix(options).catch((error) => {
    process.stderr.write(`${error.code || 'matrix_error'}: ${error.message}\n`);
    process.exitCode = interruptedSignal === 'SIGINT' ? 130
      : interruptedSignal === 'SIGTERM' ? 143 : 1;
  }).finally(() => {
    process.removeListener('SIGINT', interruptSigint);
    process.removeListener('SIGTERM', interruptSigterm);
  });
}

module.exports = {
  MATRIX_PROFILE,
  MatrixError,
  SUPPLEMENTAL_CASE_ORDER,
  buildPlan,
  failureDocument,
  fetchWorkerHealth,
  observation,
  parseArgs,
  parseCaseCatalog,
  phaseTimeoutMs,
  promptfooArguments,
  runMatrix,
  sanitize,
  terminateProcessTree,
  validateCombinedRows,
  validatePhaseDocument,
  validateWorkerHealth,
  validateWorkerUrl,
};
