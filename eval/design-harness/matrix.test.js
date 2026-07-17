const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const {
  MINIMUM_RUNS_BY_CASE_ID,
  REQUIRED_CASE_IDS,
  REQUIRED_SAMPLE_TOTAL,
} = require('./acceptance');
const {
  buildPlan,
  SUPPLEMENTAL_CASE_ORDER,
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
} = require('./matrix');

const SOURCE_COMMIT = 'a'.repeat(40);
const WORKER_SOURCE = 'b'.repeat(64);
const TOKEN = 'matrix-secret-token';
const TOOLING = Object.freeze({
  node: process.version,
  node_executable_sha256: 'c'.repeat(64),
  promptfoo: '0.121.18',
  promptfoo_entrypoint_sha256: 'd'.repeat(64),
  promptfoo_package_sha256: 'e'.repeat(64),
  package_lock_sha256: 'f'.repeat(64),
  cargo_executable: '/toolchain/cargo',
  cargo_executable_sha256: '1'.repeat(64),
  cargo_version: 'cargo 1.90.0',
});

function worker(
  instanceId = 'worker-instance',
  acceptedRequests = 100,
  settledRequests = acceptedRequests,
  activeRequests = 0,
  queuedRequests = 0,
) {
  return {
    schema_version: 1,
    status: 'ok',
    provider: 'codex_chatgpt',
    model: 'gpt-5.6-luna',
    reasoning_effort: 'medium',
    auth_mode: 'chatgpt',
    codex_cli_version: 'codex-cli 0.144.2',
    concurrency_limit: 1,
    queue_capacity: 0,
    request_timeout_ms: 55000,
    instance_id: instanceId,
    worker_source_sha256: WORKER_SOURCE,
    active_requests: activeRequests,
    queued_requests: queuedRequests,
    accepted_requests_total: acceptedRequests,
    settled_requests_total: settledRequests,
  };
}

function report(runId, runOrder, modelCalls) {
  return {
    provenance: {
      run_id: runId,
      run_order: runOrder,
      source_commit: SOURCE_COMMIT,
      build_source_commit: SOURCE_COMMIT,
      source_dirty: false,
      build_source_dirty: false,
    },
    elapsed_ms: 100,
    observability: {
      model_calls: modelCalls,
      tool_calls: 1,
      repair_attempts: 0,
    },
    model_call_metrics: [{ prompt_tokens: 20, completion_tokens: 3 }],
  };
}

function phaseDocument(phase, runId = 'matrix-run') {
  const modelCallsByCaseId = catalogAndPlan().plan.expected_model_calls_by_case_id;
  let order = phase.first_run_order;
  const rows = [];
  for (let repeat = 0; repeat < phase.repeat; repeat += 1) {
    for (const caseId of phase.case_ids) {
      rows.push({
        vars: { caseId, cohort: 'intent_recipe' },
        success: true,
        response: {
          output: '{}',
          metadata: report(runId, order, modelCallsByCaseId[caseId]),
        },
        gradingResult: { componentResults: [] },
      });
      order += 1;
    }
  }
  return { results: { results: rows } };
}

let cachedCatalogAndPlan;

function catalogAndPlan() {
  if (!cachedCatalogAndPlan) {
    const yaml = fs.readFileSync(path.join(__dirname, 'intent-cases.yaml'), 'utf8');
    const catalog = parseCaseCatalog(yaml);
    cachedCatalogAndPlan = { catalog, plan: buildPlan(catalog) };
  }
  return cachedCatalogAndPlan;
}

async function filesBelow(directory) {
  const entries = await fs.promises.readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const target = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await filesBelow(target));
    } else {
      files.push(target);
    }
  }
  return files;
}

function createRequestCounter(initial = 100) {
  let accepted = initial;
  let settled = initial;
  return {
    advance(value) {
      accepted += value;
      settled += value;
    },
    snapshot() {
      return { accepted, settled };
    },
  };
}

function dependencies(
  spawnPhase,
  instanceId = 'worker-instance',
  resultsRoot = os.tmpdir(),
  requestCounter = createRequestCounter(),
) {
  let tick = 0;
  return {
    environment: {
      STARRING_CODEX_WORKER_URL: 'http://127.0.0.1:18181',
      STARRING_CODEX_WORKER_TOKEN: TOKEN,
      OPENAI_API_KEY: 'unrelated-secret',
    },
    sourceState: () => ({ commit: SOURCE_COMMIT, dirty: false }),
    health: async () => {
      const counters = requestCounter.snapshot();
      return validateWorkerHealth(worker(instanceId, counters.accepted, counters.settled));
    },
    workerSource: async () => WORKER_SOURCE,
    spawnPhase: async (request) => {
      const result = await spawnPhase(request);
      if (await fs.promises.stat(request.output).then(() => true, () => false)) {
        const document = JSON.parse(await fs.promises.readFile(request.output, 'utf8'));
        const modelCalls = document.results.results.reduce(
          (sum, row) => sum + row.response.metadata.observability.model_calls,
          0,
        );
        requestCounter.advance(modelCalls);
      }
      return result;
    },
    runGate: async () => ({ code: 0, signal: null, stdout: '', stderr: '' }),
    tooling: () => ({ ...TOOLING }),
    resultsRoot,
    assess: () => ({ pass: true, checks: [] }),
    summarize: () => [{ case_id: 'summary' }],
    now: () => new Date(Date.UTC(2026, 6, 17, 0, 0, tick++)),
    runId: () => 'matrix-run',
    finalizerIdentity: (source) => ({
      schema_version: 1,
      source_commit: source.commit,
      matrix_sha256: '2'.repeat(64),
      acceptance_sha256: '3'.repeat(64),
      summarize_sha256: '4'.repeat(64),
    }),
    writeOutput: () => {},
    requestCounter,
  };
}

test('plan reuses one all-case smoke and adds deterministic exact-floor supplements', () => {
  const { catalog, plan } = catalogAndPlan();

  assert.deepEqual(catalog.map((entry) => entry.case_id), REQUIRED_CASE_IDS);
  assert.equal(plan.required_sample_total, REQUIRED_SAMPLE_TOTAL);
  assert.equal(REQUIRED_SAMPLE_TOTAL, 232);
  assert.equal(plan.total_expected_model_calls, 298);
  assert.equal(plan.phases.length, 27);
  assert.equal(plan.phases[0].repeat, 1);
  assert.equal(plan.phases[0].expected_rows, 26);
  assert.equal(plan.phases[0].expected_model_calls, 34);
  assert.equal(plan.phases[0].first_run_order, 1);
  assert.equal(plan.phases[0].last_run_order, 26);
  assert.equal(plan.phases[0].filter_pattern, null);
  assert.deepEqual(
    plan.phases.slice(1).map((phase) => phase.case_ids[0]),
    SUPPLEMENTAL_CASE_ORDER,
  );
  assert.deepEqual(
    plan.phases.slice(1).map((phase) => phase.repeat),
    SUPPLEMENTAL_CASE_ORDER.map((caseId) => MINIMUM_RUNS_BY_CASE_ID[caseId] - 1),
  );
  assert.ok(plan.phases.slice(1).every((phase) => phase.case_ids.length === 1));
  assert.equal(plan.phases.at(-1).last_run_order, REQUIRED_SAMPLE_TOTAL);
  assert.deepEqual(plan.minimum_runs_by_case_id, MINIMUM_RUNS_BY_CASE_ID);
  assert.match(plan.phases[1].filter_pattern, /^\^/);
  assert.match(plan.phases[1].filter_pattern, /\$$/);
  for (const phase of plan.phases.slice(1)) {
    const pattern = new RegExp(phase.filter_pattern);
    const matches = catalog.filter((entry) => pattern.test(entry.description));
    assert.deepEqual(matches.map((entry) => entry.case_id), phase.case_ids);
  }
});

test('arguments require an output and keep resume and dry-run disjoint', () => {
  const output = path.resolve('result-directory');
  assert.deepEqual(parseArgs(['--output', output]), {
    output,
    resume: false,
    dryRun: false,
  });
  assert.deepEqual(parseArgs(['--resume', '--output', output]), {
    output,
    resume: true,
    dryRun: false,
  });
  assert.throws(() => parseArgs([]), { code: 'output_required' });
  assert.throws(
    () => parseArgs(['--output', output, '--resume', '--dry-run']),
    { code: 'resume_dry_run_conflict' },
  );
  assert.throws(() => parseArgs(['--unknown']), { code: 'unknown_argument' });
});

test('worker boundary requires exact identity, idle state, and safe counter shape', () => {
  assert.equal(validateWorkerUrl('http://127.0.0.1:18181').origin, 'http://127.0.0.1:18181');
  assert.throws(
    () => validateWorkerUrl('http://localhost:18181'),
    { code: 'worker_must_be_exact_loopback' },
  );
  assert.throws(
    () => validateWorkerUrl('http://127.0.0.1:18182'),
    { code: 'worker_must_be_exact_loopback' },
  );
  assert.equal(validateWorkerHealth(worker()).instance_id, 'worker-instance');
  assert.equal(validateWorkerHealth(worker()).worker_source_sha256, WORKER_SOURCE);
  assert.equal(validateWorkerHealth(worker()).accepted_requests_total, 100);
  assert.throws(
    () => validateWorkerHealth(worker('worker-instance', 101, 100, 1, 0)),
    { code: 'worker_not_idle' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), accepted_requests_total: 1.5 }),
    { code: 'invalid_worker_request_counters' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), settled_requests_total: 101 }),
    { code: 'invalid_worker_request_counters' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), accepted_requests_total: 101 }),
    { code: 'worker_request_counter_invariant' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), concurrency_limit: 2 }),
    { code: 'worker_identity_mismatch' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), queue_capacity: 1 }),
    { code: 'worker_identity_mismatch' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), request_timeout_ms: 60000 }),
    { code: 'worker_identity_mismatch' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), worker_source_sha256: 'invalid' }),
    { code: 'invalid_worker_source_sha256' },
  );
  assert.throws(
    () => validateWorkerHealth({ ...worker(), instance_id: '' }),
    { code: 'invalid_worker_instance_id' },
  );
});

test('promptfoo arguments pin concurrency, cache, sharing, writes, repeat, output, and filter', () => {
  const { plan } = catalogAndPlan();
  const output = '/tmp/matrix-phase.json';
  const args = promptfooArguments(plan.phases[1], output);

  assert.deepEqual(args.slice(0, 2), ['eval', '-c']);
  assert.ok(args.includes('1'));
  assert.ok(args.includes('--no-cache'));
  assert.ok(args.includes('--no-share'));
  assert.ok(args.includes('--no-write'));
  assert.ok(args.includes('--no-progress-bar'));
  assert.ok(args.includes('--no-table'));
  assert.equal(args[args.indexOf('--repeat') + 1], '9');
  assert.equal(args[args.indexOf('--output') + 1], output);
  assert.equal(args[args.indexOf('--filter-pattern') + 1], plan.phases[1].filter_pattern);
  assert.equal(args.some((entry) => entry.includes(TOKEN)), false);
  const twoCallPhase = plan.phases.find((phase) => (
    phase.case_ids[0] === 'intent_private_study_room_custom_details'
  ));
  assert.equal(
    phaseTimeoutMs(twoCallPhase),
    twoCallPhase.expected_model_calls * 65000 + 30000,
  );
  assert.ok(phaseTimeoutMs(twoCallPhase) > twoCallPhase.expected_rows * 70000 + 30000);
});

test('phase and combined validation require exact cases, source, run, and contiguous orders', () => {
  const { plan } = catalogAndPlan();
  const boundary = { run_id: 'matrix-run', source_commit: SOURCE_COMMIT };
  const rows = plan.phases.flatMap((phase) => (
    validatePhaseDocument(phaseDocument(phase), phase, boundary)
  ));

  assert.equal(validateCombinedRows(rows, plan, boundary).length, REQUIRED_SAMPLE_TOTAL);

  const wrongOrder = phaseDocument(plan.phases[0]);
  wrongOrder.results.results[0].response.metadata.provenance.run_order = 2;
  assert.throws(
    () => validatePhaseDocument(wrongOrder, plan.phases[0], boundary),
    { code: 'phase_run_order_mismatch' },
  );

  const wrongSource = phaseDocument(plan.phases[0]);
  wrongSource.results.results[0].response.metadata.provenance.source_commit = 'c'.repeat(40);
  assert.throws(
    () => validatePhaseDocument(wrongSource, plan.phases[0], boundary),
    { code: 'phase_boundary_mismatch' },
  );
});

test('matrix writes atomic phase and final evidence without serializing its token', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-'));
  const output = path.join(root, 'run');
  const invocations = [];
  const gates = [];
  let progress = '';
  const spawnPhase = async (request) => {
    invocations.push(request);
    await fs.promises.writeFile(
      request.output,
      JSON.stringify(phaseDocument(request.phase, request.environment.STARRING_EVAL_RUN_ID)),
    );
    return { code: 0, signal: null, stdout: '', stderr: '' };
  };

  try {
    const overrides = dependencies(spawnPhase, 'worker-instance', root);
    overrides.runGate = async (request) => {
      gates.push(request);
      return { code: 0, signal: null, stdout: '', stderr: '' };
    };
    overrides.writeOutput = (value) => {
      progress += value;
    };
    const result = await runMatrix(
      { output, resume: false, dryRun: false },
      overrides,
    );
    assert.equal(result.status, 'passed');
    assert.equal(invocations.length, 27);
    assert.deepEqual(
      invocations.map((entry) => entry.environment.STARRING_EVAL_RUN_ORDER_OFFSET),
      catalogAndPlan().plan.phases.map((phase) => String(phase.first_run_order - 1)),
    );
    assert.ok(invocations.every((entry) => entry.environment.STARRING_EVAL_RUN_ID === 'matrix-run'));
    assert.ok(invocations.every((entry) => entry.environment.PROMPTFOO_DISABLE_TELEMETRY === '1'));
    assert.ok(invocations.every((entry) => entry.environment.PROMPTFOO_PASS_RATE_THRESHOLD === '100'));
    assert.ok(invocations.every((entry) => entry.environment.PROMPTFOO_FAILED_TEST_EXIT_CODE === '100'));
    assert.ok(invocations.every((entry) => !entry.environment.OPENAI_API_KEY));
    assert.ok(invocations.every((entry) => entry.args.every((arg) => !arg.includes(TOKEN))));
    assert.deepEqual(gates.map((entry) => [entry.command, entry.args]), [
      ['/toolchain/cargo', ['test', '-p', 'design-harness']],
      ['npm', ['test']],
    ]);
    assert.ok(gates.every((entry) => !entry.environment.STARRING_CODEX_WORKER_TOKEN));
    assert.equal(gates[0].environment.CARGO, '/toolchain/cargo');
    assert.equal(gates[0].environment.PATH.split(path.delimiter)[0], '/toolchain');
    assert.equal((progress.match(/"event":"phase_started"/g) || []).length, 27);
    assert.equal((progress.match(/"event":"phase_completed"/g) || []).length, 27);
    const state = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    const manifest = JSON.parse(
      await fs.promises.readFile(path.join(output, 'manifest.json'), 'utf8'),
    );
    const acceptance = JSON.parse(
      await fs.promises.readFile(path.join(output, 'acceptance.json'), 'utf8'),
    );
    assert.equal(state.status, 'passed');
    assert.ok(state.gates.every((gate) => gate.status === 'passed'));
    assert.equal(state.worker.instance_id, 'worker-instance');
    assert.equal(state.worker.worker_source_sha256, WORKER_SOURCE);
    assert.deepEqual(state.tooling, TOOLING);
    assert.equal(state.observed.samples, REQUIRED_SAMPLE_TOTAL);
    assert.equal(state.observed.model_calls, 298);
    assert.equal(state.observed.prompt_tokens, REQUIRED_SAMPLE_TOTAL * 20);
    assert.equal(state.observed.completion_tokens, REQUIRED_SAMPLE_TOTAL * 3);
    assert.equal(state.observed.request_counter_clean, true);
    assert.equal(state.observed.accepted_requests_delta, 298);
    assert.equal(state.observed.settled_requests_delta, 298);
    assert.deepEqual(state.request_counters, {
      initial: { accepted_requests_total: 100, settled_requests_total: 100 },
      last_observed: { accepted_requests_total: 398, settled_requests_total: 398 },
    });
    assert.ok(state.phases.every((phase) => (
      phase.request_counters.delta.accepted_requests_total === phase.expected_model_calls
        && phase.request_counters.delta.settled_requests_total === phase.expected_model_calls
    )));
    assert.equal(manifest.plan.required_sample_total, REQUIRED_SAMPLE_TOTAL);
    assert.deepEqual(manifest.request_counters, state.request_counters);
    assert.equal(manifest.observed.request_counter_clean, true);
    assert.equal(acceptance.model_acceptance_pass, true);
    assert.equal(acceptance.pass, true);
    assert.equal(acceptance.status, 'passed');
    assert.deepEqual(acceptance.certification_failures, []);
    for (const file of await filesBelow(output)) {
      const serialized = await fs.promises.readFile(file, 'utf8');
      assert.equal(serialized.includes(TOKEN), false, file);
      assert.equal(serialized.includes('127.0.0.1'), false, file);
    }
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('matrix rejects an external worker request before the first phase', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-before-'));
  const output = path.join(root, 'run');
  const requestCounter = createRequestCounter();
  let spawned = false;
  let gateCalls = 0;
  const overrides = dependencies(async () => {
    spawned = true;
    throw new Error('not reached');
  }, 'worker-instance', root, requestCounter);
  overrides.runGate = async () => {
    gateCalls += 1;
    if (gateCalls === 1) {
      requestCounter.advance(1);
    }
    return { code: 0, signal: null, stdout: '', stderr: '', timed_out: false };
  };
  try {
    await assert.rejects(
      runMatrix({ output, resume: false, dryRun: false }, overrides),
      { code: 'request_counter_discontinuity' },
    );
    assert.equal(spawned, false);
    const state = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(state.phases[0].status, 'failed');
    assert.equal(state.phases[0].error_code, 'request_counter_discontinuity');
    assert.deepEqual(state.request_counters.last_observed, {
      accepted_requests_total: 100,
      settled_requests_total: 100,
    });
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('matrix rejects an external worker request across resume', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-resume-gap-'));
  const output = path.join(root, 'run');
  const requestCounter = createRequestCounter();
  const firstSpawn = async (request) => {
    if (request.phase.index === 1) {
      return { code: 1, signal: null, stdout: '', stderr: 'stop', timed_out: false };
    }
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    return { code: 0, signal: null, stdout: '', stderr: '', timed_out: false };
  };
  try {
    await assert.rejects(
      runMatrix(
        { output, resume: false, dryRun: false },
        dependencies(firstSpawn, 'worker-instance', root, requestCounter),
      ),
      { code: 'promptfoo_phase_failed' },
    );
    requestCounter.advance(1);
    await assert.rejects(
      runMatrix(
        { output, resume: true, dryRun: false },
        dependencies(async () => {
          throw new Error('not reached');
        }, 'worker-instance', root, requestCounter),
      ),
      { code: 'resume_request_counter_mismatch' },
    );
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('matrix rejects an extra worker request during a phase', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-during-'));
  const output = path.join(root, 'run');
  const requestCounter = createRequestCounter();
  const spawnPhase = async (request) => {
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    requestCounter.advance(1);
    return { code: 0, signal: null, stdout: '', stderr: '', timed_out: false };
  };
  try {
    await assert.rejects(
      runMatrix(
        { output, resume: false, dryRun: false },
        dependencies(spawnPhase, 'worker-instance', root, requestCounter),
      ),
      { code: 'request_counter_delta_mismatch' },
    );
    const state = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(state.phases[0].expected_model_calls, 34);
    assert.deepEqual(state.phases[0].request_counters.delta, {
      accepted_requests_total: 35,
      settled_requests_total: 35,
    });
    assert.deepEqual(state.request_counters.last_observed, {
      accepted_requests_total: 100,
      settled_requests_total: 100,
    });
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('resume preserves completed evidence and retries only unfinished phases on the same boundary', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-resume-'));
  const output = path.join(root, 'run');
  let calls = 0;
  const firstSpawn = async (request) => {
    calls += 1;
    if (request.phase.index === 1) {
      return { code: 1, signal: null, stdout: '', stderr: `failed ${TOKEN}` };
    }
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    return { code: 0, signal: null, stdout: '', stderr: '' };
  };
  const requestCounter = createRequestCounter();

  try {
    await assert.rejects(
      runMatrix(
        { output, resume: false, dryRun: false },
        dependencies(firstSpawn, 'worker-instance', root, requestCounter),
      ),
      { code: 'promptfoo_phase_failed' },
    );
    assert.equal(calls, 2);
    const failed = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.deepEqual(failed.phases.slice(0, 3).map((phase) => phase.status), [
      'completed',
      'failed',
      'pending',
    ]);
    assert.ok(failed.phases.slice(2).every((phase) => phase.status === 'pending'));
    assert.equal(failed.phases[1].error_detail.includes(TOKEN), false);
    assert.equal(failed.phases[1].error_detail.includes('[REDACTED]'), true);
    const failureArtifact = JSON.parse(
      await fs.promises.readFile(path.join(output, 'failures.json'), 'utf8'),
    );
    assert.equal(failureArtifact.matrix_error.code, 'promptfoo_phase_failed');
    assert.equal(JSON.stringify(failureArtifact).includes(TOKEN), false);

    const tampered = structuredClone(failed);
    tampered.plan.profile = 'tampered-profile';
    await fs.promises.writeFile(path.join(output, 'state.json'), JSON.stringify(tampered));
    await assert.rejects(
      runMatrix(
        { output, resume: true, dryRun: false },
        dependencies(async () => {
          throw new Error('not reached');
        }, 'worker-instance', root, requestCounter),
      ),
      { code: 'resume_boundary_mismatch' },
    );
    await fs.promises.writeFile(path.join(output, 'state.json'), JSON.stringify(failed));
    const orphan = path.join(output, 'phases', 'orphan-attempt.json');
    const staleTemporary = path.join(output, 'phases', 'stale.tmp.json');
    await fs.promises.writeFile(orphan, '{}');
    await fs.promises.writeFile(staleTemporary, '{}');

    const resumed = [];
    const resumeSpawn = async (request) => {
      resumed.push(request.phase.index);
      await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
      return { code: 0, signal: null, stdout: '', stderr: '' };
    };
    await assert.rejects(
      runMatrix(
        { output, resume: true, dryRun: false },
        dependencies(resumeSpawn, 'worker-instance', root, requestCounter),
      ),
      { code: 'acceptance_matrix_failed' },
    );
    assert.deepEqual(resumed, Array.from({ length: 26 }, (_, index) => index + 1));
    const state = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(state.status, 'failed');
    assert.equal(state.observed.retry_free, false);
    const retriedAcceptance = JSON.parse(
      await fs.promises.readFile(path.join(output, 'acceptance.json'), 'utf8'),
    );
    assert.equal(retriedAcceptance.model_acceptance_pass, true);
    assert.equal(retriedAcceptance.pass, false);
    assert.equal(retriedAcceptance.status, 'failed');
    assert.deepEqual(retriedAcceptance.certification_failures, ['phase_retry_detected']);
    assert.equal(fs.existsSync(orphan), false);
    assert.equal(fs.existsSync(staleTemporary), false);
    assert.deepEqual(state.phases.slice(0, 3).map((phase) => phase.attempts), [1, 2, 1]);
    assert.ok(state.phases.slice(2).every((phase) => phase.attempts === 1));

    await assert.rejects(
      runMatrix(
        { output, resume: true, dryRun: false },
        dependencies(resumeSpawn, 'replacement-worker', root, requestCounter),
      ),
      { code: 'cohort_already_finalized' },
    );
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('a complete Promptfoo assertion-failure phase is preserved without retry or selection bias', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-assertion-'));
  const output = path.join(root, 'run');
  const invoked = [];
  const spawnPhase = async (request) => {
    invoked.push(request.phase.index);
    const document = phaseDocument(request.phase);
    if (request.phase.index === 0) {
      const naming = document.results.results.find((row) => (
        row.vars.caseId === 'intent_private_study_room_mutation_naming'
      ));
      naming.response.metadata.observability.model_calls = 1;
    }
    if (request.phase.index === 1) {
      document.results.results[0].success = false;
      document.results.results[0].gradingResult.componentResults = [{
        pass: false,
        assertion: { value: 'file://intent-assertions.js:taskSemantics' },
      }];
    }
    await fs.promises.writeFile(request.output, JSON.stringify(document));
    return {
      code: request.phase.index === 1 ? 100 : 0,
      signal: null,
      stdout: '',
      stderr: '',
    };
  };

  try {
    await assert.rejects(
      runMatrix(
        { output, resume: false, dryRun: false },
        dependencies(spawnPhase, 'worker-instance', root),
      ),
      { code: 'acceptance_matrix_failed' },
    );
    assert.equal(invoked.length, 27);
    const state = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.ok(state.phases.every((phase) => phase.status === 'completed'));
    assert.ok(state.phases.every((phase) => phase.attempts === 1));
    assert.equal(state.phases[0].expected_model_calls, 34);
    assert.equal(state.phases[0].actual_model_calls, 33);
    assert.equal(state.phases[1].promptfoo_exit_code, 100);
    assert.equal(state.observed.retry_free, true);
    assert.equal(state.observed.promptfoo_clean_exit, false);
    assert.equal(state.observed.request_counter_clean, true);
    assert.equal(state.observed.model_call_plan_met, false);
    assert.equal(state.observed.model_calls, 297);
    assert.equal(state.observed.expected_model_calls, 298);
    const acceptance = JSON.parse(
      await fs.promises.readFile(path.join(output, 'acceptance.json'), 'utf8'),
    );
    assert.equal(acceptance.model_acceptance_pass, true);
    assert.equal(acceptance.pass, false);
    assert.deepEqual(acceptance.certification_failures, [
      'promptfoo_nonzero_exit',
      'model_call_plan_mismatch',
    ]);
    const failures = JSON.parse(
      await fs.promises.readFile(path.join(output, 'failures.json'), 'utf8'),
    );
    assert.deepEqual(failures.certification_failures, [
      'promptfoo_nonzero_exit',
      'model_call_plan_mismatch',
    ]);
    assert.equal(failures.failures.length, 1);
    assert.deepEqual(failures.failures[0].failed_assertions, ['taskSemantics']);
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('a completed cohort can be finalized by a separately attested committed evaluator', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-finalize-'));
  const output = path.join(root, 'run');
  const requestCounter = createRequestCounter();
  let calls = 0;
  const spawnPhase = async (request) => {
    calls += 1;
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    return { code: 0, signal: null, stdout: '', stderr: '' };
  };

  try {
    const first = dependencies(spawnPhase, 'worker-instance', root, requestCounter);
    first.assess = () => {
      throw new Error('aggregation defect');
    };
    await assert.rejects(
      runMatrix({ output, resume: false, dryRun: false }, first),
      /aggregation defect/,
    );
    assert.equal(calls, 27);
    const failed = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(failed.status, 'failed');
    assert.ok(failed.phases.every((phase) => phase.status === 'completed'));
    assert.equal(failed.failure_evidence.path, 'failures.json');
    assert.equal(failed.failure_evidence.document.matrix_error.detail, 'aggregation defect');

    const finalizerCommit = 'b'.repeat(40);
    const tampered = structuredClone(failed);
    tampered.failure_evidence.document.matrix_error.detail = 'changed after failure';
    await fs.promises.writeFile(path.join(output, 'state.json'), JSON.stringify(tampered));
    const tamperedFinalizer = dependencies(async () => {
      throw new Error('completed phases must not rerun');
    }, 'worker-instance', root, requestCounter);
    tamperedFinalizer.sourceState = () => ({ commit: finalizerCommit, dirty: false });
    await assert.rejects(
      runMatrix({ output, resume: true, dryRun: false }, tamperedFinalizer),
      { code: 'deferred_finalization_failure_evidence_missing' },
    );
    await fs.promises.writeFile(path.join(output, 'state.json'), JSON.stringify(failed));
    const second = dependencies(async () => {
      throw new Error('completed phases must not rerun');
    }, 'worker-instance', root, requestCounter);
    second.sourceState = () => ({ commit: finalizerCommit, dirty: false });
    let healthCalls = 0;
    const health = second.health;
    second.health = async (...args) => {
      healthCalls += 1;
      return health(...args);
    };
    second.runGate = async () => {
      throw new Error('completed gates must not rerun');
    };
    const result = await runMatrix({ output, resume: true, dryRun: false }, second);

    assert.equal(result.status, 'passed');
    assert.equal(calls, 27);
    assert.equal(healthCalls, 2);
    const manifest = JSON.parse(
      await fs.promises.readFile(path.join(output, 'manifest.json'), 'utf8'),
    );
    const acceptance = JSON.parse(
      await fs.promises.readFile(path.join(output, 'acceptance.json'), 'utf8'),
    );
    assert.equal(manifest.source_commit, SOURCE_COMMIT);
    assert.equal(manifest.finalizer.source_commit, finalizerCommit);
    assert.equal(manifest.finalizer.mode, 'deferred_completed_phase_artifacts');
    assert.equal(manifest.finalizer.model_requests_executed, 0);
    assert.equal(acceptance.evidence_source_commit, SOURCE_COMMIT);
    assert.equal(acceptance.finalizer.source_commit, finalizerCommit);
    const recovery = JSON.parse(
      await fs.promises.readFile(
        path.join(output, manifest.finalizer.recovery_input.path),
        'utf8',
      ),
    );
    assert.equal(recovery.document.matrix_error.detail, 'aggregation defect');
    assert.equal(manifest.artifacts.recovery_input.sha256, manifest.finalizer.recovery_input.sha256);
    await assert.rejects(
      runMatrix({ output, resume: true, dryRun: false }, second),
      { code: 'cohort_already_finalized' },
    );
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('deferred finalization resumes after interruption without losing bound failure evidence', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-recovery-'));
  const output = path.join(root, 'run');
  const requestCounter = createRequestCounter();
  const spawnPhase = async (request) => {
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    return { code: 0, signal: null, stdout: '', stderr: '' };
  };

  try {
    const first = dependencies(spawnPhase, 'worker-instance', root, requestCounter);
    first.assess = () => {
      throw new Error('bound aggregation defect');
    };
    first.beforeFailureArtifactWrite = async () => {
      throw new Error('failure artifact interruption');
    };
    await assert.rejects(
      runMatrix({ output, resume: false, dryRun: false }, first),
      /failure artifact interruption/,
    );
    const failed = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    const boundDigest = failed.failure_evidence.sha256;
    assert.equal(fs.existsSync(path.join(output, 'failures.json')), false);
    const finalizerCommit = 'b'.repeat(40);
    const interrupted = dependencies(async () => {
      throw new Error('completed phases must not rerun');
    }, 'worker-instance', root, requestCounter);
    interrupted.sourceState = () => ({ commit: finalizerCommit, dirty: false });
    interrupted.beforeFinalStateWrite = async () => {
      throw new Error('final state interruption');
    };
    await assert.rejects(
      runMatrix({ output, resume: true, dryRun: false }, interrupted),
      /final state interruption/,
    );
    const afterInterruption = JSON.parse(
      await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'),
    );
    assert.equal(afterInterruption.completed_at, null);
    assert.equal(afterInterruption.failure_evidence.sha256, boundDigest);
    const overwrittenFailure = JSON.parse(
      await fs.promises.readFile(path.join(output, 'failures.json'), 'utf8'),
    );
    assert.equal(overwrittenFailure.matrix_error, undefined);

    const resumed = dependencies(async () => {
      throw new Error('completed phases must not rerun');
    }, 'worker-instance', root, requestCounter);
    resumed.sourceState = () => ({ commit: finalizerCommit, dirty: false });
    const result = await runMatrix({ output, resume: true, dryRun: false }, resumed);
    assert.equal(result.status, 'passed');
    const recovery = JSON.parse(
      await fs.promises.readFile(
        path.join(output, 'recovery', 'prior-finalization-failure.json'),
        'utf8',
      ),
    );
    assert.equal(recovery.original_sha256, boundDigest);
    assert.equal(recovery.document.matrix_error.detail, 'bound aggregation defect');
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('a resumed deterministic gate retry remains visible and cannot certify', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-gate-'));
  const output = path.join(root, 'run');
  const spawnPhase = async (request) => {
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    return { code: 0, signal: null, stdout: '', stderr: '', timed_out: false };
  };
  const first = dependencies(spawnPhase, 'worker-instance', root);
  first.runGate = async () => ({
    code: 1,
    signal: null,
    stdout: '',
    stderr: 'gate failed',
    timed_out: false,
  });
  try {
    await assert.rejects(
      runMatrix({ output, resume: false, dryRun: false }, first),
      { code: 'deterministic_gate_failed' },
    );
    const failed = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(failed.gates[0].status, 'failed');
    assert.equal(failed.gates[0].attempts, 1);

    await assert.rejects(
      runMatrix(
        { output, resume: true, dryRun: false },
        dependencies(spawnPhase, 'worker-instance', root),
      ),
      { code: 'acceptance_matrix_failed' },
    );
    const resumed = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(resumed.gates[0].status, 'passed');
    assert.equal(resumed.gates[0].attempts, 2);
    assert.equal(resumed.observed.deterministic_gates_retry_free, false);
    const failures = JSON.parse(
      await fs.promises.readFile(path.join(output, 'failures.json'), 'utf8'),
    );
    assert.deepEqual(failures.certification_failures, ['deterministic_gate_retry_detected']);
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('dry run validates a clean source and prints the exact plan without worker access', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-dry-'));
  const output = path.join(root, 'unused');
  let printed = '';
  let healthCalled = false;
  try {
    const result = await runMatrix(
      { output, resume: false, dryRun: true },
      {
        sourceState: () => ({ commit: SOURCE_COMMIT, dirty: false }),
        workerSource: async () => WORKER_SOURCE,
        tooling: () => ({ ...TOOLING }),
        resultsRoot: root,
        health: async () => {
          healthCalled = true;
          return validateWorkerHealth(worker());
        },
        writeOutput: (value) => {
          printed += value;
        },
      },
    );
    assert.equal(result.status, 'dry_run');
    assert.equal(result.plan.required_sample_total, REQUIRED_SAMPLE_TOTAL);
    assert.equal(healthCalled, false);
    assert.match(printed, new RegExp(SOURCE_COMMIT));
    assert.equal(fs.existsSync(output), false);
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('matrix rejects a local worker source mismatch before creating evidence', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-source-'));
  const output = path.join(root, 'run');
  try {
    await assert.rejects(
      runMatrix(
        { output, resume: false, dryRun: false },
        {
          ...dependencies(async () => {
            throw new Error('not reached');
          }, 'worker-instance', root),
          workerSource: async () => 'c'.repeat(64),
        },
      ),
      { code: 'worker_source_not_local' },
    );
    assert.equal(fs.existsSync(output), false);
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('matrix removes and rejects a phase artifact containing its bearer token', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-secret-'));
  const output = path.join(root, 'run');
  const spawnPhase = async (request) => {
    const document = phaseDocument(request.phase);
    document.results.results[0].response.error = TOKEN;
    await fs.promises.writeFile(request.output, JSON.stringify(document));
    return { code: 0, signal: null, stdout: '', stderr: '' };
  };
  try {
    await assert.rejects(
      runMatrix(
        { output, resume: false, dryRun: false },
        dependencies(spawnPhase, 'worker-instance', root),
      ),
      { code: 'secret_material_detected' },
    );
    for (const file of await filesBelow(output)) {
      const serialized = await fs.promises.readFile(file, 'utf8');
      assert.equal(serialized.includes(TOKEN), false, file);
    }
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('resume scrubs secret orphans before rejecting a changed worker boundary', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-orphan-'));
  const output = path.join(root, 'run');
  const requestCounter = createRequestCounter();
  const overrides = dependencies(
    async () => ({ code: 1, signal: null, stdout: '', stderr: 'failed' }),
    'worker-instance',
    root,
    requestCounter,
  );
  const orphan = path.join(output, 'phases', 'orphan.attempt-01.tmp.json');
  try {
    await assert.rejects(
      runMatrix({ output, resume: false, dryRun: false }, overrides),
      { code: 'promptfoo_phase_failed' },
    );
    await fs.promises.writeFile(orphan, JSON.stringify({ token: TOKEN }), { mode: 0o600 });
    requestCounter.advance(1);

    await assert.rejects(
      runMatrix({ output, resume: true, dryRun: false }, overrides),
      { code: 'secret_material_detected' },
    );
    assert.equal(fs.existsSync(orphan), false);
    await assert.rejects(
      runMatrix({ output, resume: true, dryRun: false }, overrides),
      { code: 'resume_request_counter_mismatch' },
    );
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('matrix fails a shard when the committed source changes after execution', async () => {
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'starring-matrix-boundary-'));
  const output = path.join(root, 'run');
  let sourceChecks = 0;
  const spawnPhase = async (request) => {
    await fs.promises.writeFile(request.output, JSON.stringify(phaseDocument(request.phase)));
    return { code: 0, signal: null, stdout: '', stderr: '' };
  };
  const overrides = dependencies(spawnPhase, 'worker-instance', root);
  overrides.sourceState = () => {
    sourceChecks += 1;
    return { commit: SOURCE_COMMIT, dirty: sourceChecks >= 7 };
  };
  try {
    await assert.rejects(
      runMatrix({ output, resume: false, dryRun: false }, overrides),
      { code: 'source_boundary_changed' },
    );
    const state = JSON.parse(await fs.promises.readFile(path.join(output, 'state.json'), 'utf8'));
    assert.equal(state.phases[0].status, 'failed');
    assert.equal(state.phases[0].error_code, 'source_boundary_changed');
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
});

test('sanitization removes every complete token occurrence and bounds diagnostics', () => {
  const value = `${TOKEN}-${'x'.repeat(5000)}-${TOKEN}`;
  const sanitized = sanitize(value, TOKEN);
  assert.equal(sanitized.includes(TOKEN), false);
  assert.ok(sanitized.length <= 4096);
});

test('process termination targets the child process group and falls back safely', () => {
  const originalKill = process.kill;
  const signals = [];
  const childSignals = [];
  try {
    process.kill = (pid, signal) => {
      signals.push({ pid, signal });
      return true;
    };
    terminateProcessTree({ pid: 4321, kill: (signal) => childSignals.push(signal) }, 'SIGTERM');
    assert.deepEqual(signals, [{ pid: -4321, signal: 'SIGTERM' }]);
    assert.deepEqual(childSignals, []);

    process.kill = () => {
      throw new Error('group unavailable');
    };
    terminateProcessTree({ pid: 4321, kill: (signal) => childSignals.push(signal) }, 'SIGKILL');
    assert.deepEqual(childSignals, ['SIGKILL']);
    assert.doesNotThrow(() => terminateProcessTree({ pid: null, kill() {} }, 'SIGTERM'));
  } finally {
    process.kill = originalKill;
  }
});
