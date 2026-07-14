const test = require('node:test');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const DesignHarnessProvider = require('./provider');
const {
  bindingDocument,
  cargoBuildEnvironment,
  cargoExecutable,
  gatewayIdentity,
  hydratePrompt,
  preparePrompt,
} = require('./provider');
const fixtures = require('./fixtures.json');

function executable(body) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'starring-provider-'));
  const file = path.join(directory, 'harness');
  fs.writeFileSync(file, `#!/bin/sh\n${body}\n`, { mode: 0o755 });
  return file;
}

async function call(binary, config = {}, prompt = 'test prompt') {
  const previous = {
    binary: process.env.STARRING_HARNESS_BIN,
    baseUrl: process.env.STARRING_LLM_BASE_URL,
    apiKey: process.env.STARRING_LLM_API_KEY,
    timeoutMs: process.env.STARRING_EVAL_TIMEOUT_MS,
  };
  process.env.STARRING_HARNESS_BIN = binary;
  process.env.STARRING_LLM_BASE_URL = 'http://127.0.0.1:1/v1';
  process.env.STARRING_LLM_API_KEY = 'test-only';
  try {
    return await new DesignHarnessProvider({ config }).callApi(prompt);
  } finally {
    for (const [name, value] of [
      ['STARRING_HARNESS_BIN', previous.binary],
      ['STARRING_LLM_BASE_URL', previous.baseUrl],
      ['STARRING_LLM_API_KEY', previous.apiKey],
      ['STARRING_EVAL_TIMEOUT_MS', previous.timeoutMs],
    ]) {
      if (value === undefined) {
        delete process.env[name];
      } else {
        process.env[name] = value;
      }
    }
  }
}

test('provider parses a successful single JSON report', async () => {
  const response = await call(executable('cat >/dev/null\nprintf \'%s\\n\' \'{"schema_version":1}\''));
  assert.equal(JSON.parse(response.output).schema_version, 1);
  assert.equal(response.metadata.schema_version, 1);
});

test('provider reports nonzero and invalid JSON exits', async () => {
  const failed = await call(executable('cat >/dev/null\necho failed >&2\nexit 9'));
  assert.equal(failed.error, 'failed');
  const invalid = await call(executable('cat >/dev/null\necho not-json'));
  assert.match(invalid.error, /^invalid design harness JSON:/);
});

test('provider waits for timeout termination and caps output', async () => {
  const timedOut = await call(executable("trap '' TERM\ncat >/dev/null\nwhile :; do sleep 1; done"), { timeoutMs: 30, killGraceMs: 10 });
  assert.match(timedOut.error, /timed out after 30 milliseconds/);
  const oversized = await call(executable("cat >/dev/null\nprintf '123456789'"), { maxOutputBytes: 4, killGraceMs: 10 });
  assert.match(oversized.error, /stdout exceeded 4 bytes/);
});

test('provider accepts a positive timeout from the environment', async () => {
  const previous = process.env.STARRING_EVAL_TIMEOUT_MS;
  process.env.STARRING_EVAL_TIMEOUT_MS = '30';
  try {
    const timedOut = await call(executable("trap '' TERM\ncat >/dev/null\nwhile :; do sleep 1; done"), { killGraceMs: 10 });
    assert.match(timedOut.error, /timed out after 30 milliseconds/);
  } finally {
    if (previous === undefined) {
      delete process.env.STARRING_EVAL_TIMEOUT_MS;
    } else {
      process.env.STARRING_EVAL_TIMEOUT_MS = previous;
    }
  }
});

test('provider preserves oracle brief and hydrates exact Draft and plan fixtures', () => {
  const hydrated = JSON.parse(hydratePrompt(JSON.stringify({
    schema_version: 2,
    mode: 'typed_plan',
    initial_draft: { $fixture: 'studyroom_before_resources' },
    turns: [{
      id: 'resources',
      input: 'Build resources',
      oracle_brief: {
        intent: 'build',
        objective: 'Build resources',
        requested_outcome: 'draft_update',
        assumptions: [],
        validate: false,
      },
      oracle_plan: { $fixture: 'studyroom_resources_plan' },
    }],
  })));

  assert.equal(hydrated.initial_draft.draft_revision, 5);
  assert.equal(hydrated.initial_draft.ruleset.rules[0].key, 'open_modal');
  assert.equal(hydrated.turns[0].oracle_brief.intent, 'build');
  assert.equal(hydrated.turns[0].oracle_brief.validate, false);
  assert.equal(hydrated.turns[0].oracle_plan.requirements.length, 7);
  assert.equal(hydrated.turns[0].oracle_plan.requirements[4].action.deny[0], 'view_channel');
});

test('provider rejects unknown fixture names', () => {
  assert.throws(
    () => hydratePrompt('{"schema_version":2,"initial_draft":{"$fixture":"missing"}}'),
    /unknown evaluation fixture missing/,
  );
});

test('intent provider accepts only strict schema three documents without fixtures or controls', () => {
  const document = {
    schema_version: 3,
    mode: 'intent_recipe',
    turns: [{ id: 'build', input: 'Build a private study room', restart_after: false }],
  };
  const prepared = preparePrompt(JSON.stringify(document), true);

  assert.equal(prepared.intent, true);
  assert.deepEqual(JSON.parse(prepared.prompt), document);
  for (const forbidden of [
    { ...document, initial_draft: {} },
    { ...document, turns: [{ id: 'build', input: 'Build', oracle_brief: {} }] },
    { ...document, turns: [{ id: 'build', input: 'Build', oracle_plan: {} }] },
    { ...document, turns: [{ id: 'build', input: 'Build', nested: { $fixture: 'x' } }] },
  ]) {
    assert.throws(() => preparePrompt(JSON.stringify(forbidden), true), /forbids|exactly/);
  }
  assert.throws(() => preparePrompt('plain text', true), /schema_version 3 JSON/);
  assert.throws(
    () => preparePrompt('{"schema_version":2,"mode":"typed_plan","turns":[]}', true),
    /schema_version 3/,
  );
  const duplicate = '{"schema_version":3,"schema_version":3,"mode":"intent_recipe","turns":[{"id":"a","input":"b"}]}';
  assert.equal(preparePrompt(duplicate, true).prompt, duplicate);
});

test('intent binding DTO mirrors the strict Rust environment contract', () => {
  const bindings = {
    schema_version: 1,
    channel_bindings: [{ key: 'community_hub', id: '700' }],
    role_bindings: [{ key: 'member', id: '701' }],
  };

  assert.deepEqual(JSON.parse(bindingDocument(bindings)), bindings);
  assert.throws(
    () => bindingDocument({ ...bindings, channel_bindings: [] }),
    /at least one channel/,
  );
  assert.throws(
    () => bindingDocument({ ...bindings, role_bindings: [{ key: 'member', id: '700' }] }),
    /duplicate intent binding Discord ID/,
  );
  assert.throws(
    () => bindingDocument({ ...bindings, extra: true }),
    /must contain exactly/,
  );
  assert.match(gatewayIdentity('https://llm-api.starring.co.kr/v1/'), /^sha256-[0-9a-f]{64}$/);
  assert.equal(
    gatewayIdentity('https://llm-api.starring.co.kr/v1/'),
    gatewayIdentity('https://llm-api.starring.co.kr/v1'),
  );
  assert.throws(() => gatewayIdentity('https://user:secret@example.com/v1'), /without credentials/);
});

test('intent checkpoint resolves cargo through the active rustup toolchain', () => {
  const cargo = cargoExecutable();
  assert.equal(path.basename(cargo), 'cargo');
  assert.match(execFileSync('rustc', ['-vV'], {
    encoding: 'utf8',
    env: cargoBuildEnvironment(cargo),
  }), /release:/);
});

test('intent provider pins Gemma4 and passes bindings and provenance only through env', async () => {
  const binary = executable([
    'read payload',
    'payload_b64=$(printf %s "$payload" | base64 | tr -d \'\\n\')',
    'printf \'{"payload_b64":"%s","model":"%s","mode":"%s","bindings":%s,"gateway":"%s","declared_context":"%s","commit":"%s","dirty":"%s","binary":"%s","run_id":"%s","run_order":"%s","max_model":"%s","max_tool":"%s","max_gate":"%s","context_chars":"%s"}\\n\' "$payload_b64" "$STARRING_LLM_MODEL" "$STARRING_HARNESS_MODE" "$STARRING_HARNESS_BINDINGS_JSON" "$STARRING_EVAL_GATEWAY_ID" "$STARRING_EVAL_DECLARED_CONTEXT_TOKENS" "$STARRING_EVAL_SOURCE_COMMIT" "$STARRING_EVAL_SOURCE_DIRTY" "$STARRING_EVAL_BINARY_SHA256" "$STARRING_EVAL_RUN_ID" "$STARRING_EVAL_RUN_ORDER" "$STARRING_HARNESS_MAX_MODEL_CALLS" "$STARRING_HARNESS_MAX_TOOL_CALLS" "$STARRING_HARNESS_MAX_GATE_FAILURES" "$STARRING_HARNESS_CONTEXT_CHARS"',
  ].join('\n'));
  const input = JSON.stringify({
    schema_version: 3,
    mode: 'intent_recipe',
    turns: [{ id: 'build', input: 'Build a private study room' }],
  });
  const response = await call(binary, {
    intentOnly: true,
    allowHarnessOverrideForTest: true,
    model: 'gemma4:12b-mlx',
    bindings: {
      schema_version: 1,
      channel_bindings: [{ key: 'community_hub', id: '700' }],
    },
  }, input);
  assert.equal(response.error, undefined);
  const metadata = response.metadata;

  assert.equal(Buffer.from(metadata.payload_b64, 'base64').toString('utf8'), input);
  assert.equal(metadata.model, 'gemma4:12b-mlx');
  assert.equal(metadata.mode, 'intent_recipe');
  assert.deepEqual(metadata.bindings, {
    schema_version: 1,
    channel_bindings: [{ key: 'community_hub', id: '700' }],
    role_bindings: [],
  });
  assert.match(metadata.gateway, /^sha256-[0-9a-f]{64}$/);
  assert.equal(metadata.declared_context, '16384');
  assert.match(metadata.commit, /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/);
  assert.match(metadata.dirty, /^(true|false)$/);
  assert.match(metadata.binary, /^[0-9a-f]{64}$/);
  assert.match(metadata.run_id, /^intent-/);
  assert.match(metadata.run_order, /^[1-9][0-9]*$/);
  assert.equal(metadata.max_model, '12');
  assert.equal(metadata.max_tool, '24');
  assert.equal(metadata.max_gate, '4');
  assert.equal(metadata.context_chars, '44000');

  const wrongModel = await call(binary, {
    intentOnly: true,
    allowHarnessOverrideForTest: true,
    model: 'qwen3.5:9b-mlx',
    bindings: {
      schema_version: 1,
      channel_bindings: [{ key: 'community_hub', id: '700' }],
    },
  }, input);
  assert.match(wrongModel.error, /model must be exactly gemma4:12b-mlx/);
});

test('intent checkpoint forbids an alternate harness executable', async () => {
  const input = JSON.stringify({
    schema_version: 3,
    mode: 'intent_recipe',
    turns: [{ id: 'build', input: 'Build a private study room' }],
  });
  const response = await call(executable('cat >/dev/null'), {
    intentOnly: true,
    model: 'gemma4:12b-mlx',
    bindings: {
      schema_version: 1,
      channel_bindings: [{ key: 'community_hub', id: '700' }],
    },
  }, input);

  assert.match(response.error, /STARRING_HARNESS_BIN is forbidden/);
});

test('full StudyRoom fixture is the exact ordered composition of stage plans', () => {
  const expected = [
    ...fixtures.studyroom_surface_plan.requirements,
    ...fixtures.studyroom_open_rule_plan.requirements,
    ...fixtures.studyroom_resources_plan.requirements,
    ...fixtures.studyroom_finalize_plan.requirements,
  ];

  assert.deepEqual(fixtures.studyroom_full_plan.requirements, expected);
  assert.equal(fixtures.studyroom_surface_plan.requirements.length, 3);
  assert.equal(fixtures.studyroom_open_rule_plan.requirements.length, 2);
  assert.equal(fixtures.studyroom_full_plan.requirements.length, 16);
});
