const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const DesignHarnessProvider = require('./provider');
const { hydratePrompt } = require('./provider');
const fixtures = require('./fixtures.json');

function executable(body) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'starring-provider-'));
  const file = path.join(directory, 'harness');
  fs.writeFileSync(file, `#!/bin/sh\n${body}\n`, { mode: 0o755 });
  return file;
}

async function call(binary, config = {}) {
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
    return await new DesignHarnessProvider({ config }).callApi('test prompt');
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
