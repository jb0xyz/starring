const { createHash, randomUUID } = require('node:crypto');
const { execFileSync, spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const fixtures = require('./fixtures.json');

const INTENT_MODEL = 'gpt-5.6-luna';
const INTENT_REASONING_EFFORT = 'medium';
const INTENT_CONTEXT_TOKENS = 16384;
const INTENT_TIMEOUT_MS = 60000;
const INTENT_SESSION_CONFIG = Object.freeze({
  maxModelCalls: 12,
  maxToolCalls: 24,
  maxGateFailures: 4,
  contextChars: 44000,
});
const UINT64_MAX = 18446744073709551615n;
const intentRunId = process.env.STARRING_EVAL_RUN_ID || `intent-${randomUUID()}`;
let intentRunOrder = 0;

function hydrateFixtures(value) {
  if (Array.isArray(value)) {
    return value.map(hydrateFixtures);
  }
  if (!value || typeof value !== 'object') {
    return value;
  }
  const keys = Object.keys(value);
  if (keys.length === 1 && keys[0] === '$fixture') {
    const name = value.$fixture;
    if (typeof name !== 'string' || !Object.hasOwn(fixtures, name)) {
      throw new Error(`unknown evaluation fixture ${String(name)}`);
    }
    return structuredClone(fixtures[name]);
  }
  return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, hydrateFixtures(entry)]));
}

function exactKeys(value, expected, location) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${location} must contain exactly ${wanted.join(', ')}`);
  }
}

function rejectIntentFixtures(value, location = '$') {
  if (Array.isArray(value)) {
    value.forEach((entry, index) => rejectIntentFixtures(entry, `${location}[${index}]`));
    return;
  }
  if (!value || typeof value !== 'object') {
    return;
  }
  for (const [key, entry] of Object.entries(value)) {
    if (['$fixture', 'initial_draft', 'oracle_brief', 'oracle_plan'].includes(key)) {
      throw new Error(`intent evaluation forbids ${location}.${key}`);
    }
    rejectIntentFixtures(entry, `${location}.${key}`);
  }
}

function validateIntentDocument(document) {
  rejectIntentFixtures(document);
  exactKeys(document, ['schema_version', 'mode', 'turns'], 'intent evaluation document');
  if (document.schema_version !== 3) {
    throw new Error('intent evaluation requires schema_version 3');
  }
  if (document.mode !== 'intent_recipe') {
    throw new Error('intent evaluation requires mode intent_recipe');
  }
  if (!Array.isArray(document.turns) || document.turns.length === 0) {
    throw new Error('intent evaluation requires at least one turn');
  }
  const ids = new Set();
  for (const [index, turn] of document.turns.entries()) {
    if (!turn || typeof turn !== 'object' || Array.isArray(turn)) {
      throw new Error(`intent evaluation turn ${index + 1} must be an object`);
    }
    const allowed = turn.restart_after === undefined
      ? ['id', 'input']
      : ['id', 'input', 'restart_after'];
    exactKeys(turn, allowed, `intent evaluation turn ${index + 1}`);
    if (typeof turn.id !== 'string' || turn.id.trim().length === 0) {
      throw new Error(`intent evaluation turn ${index + 1} requires a non-empty id`);
    }
    if (ids.has(turn.id)) {
      throw new Error(`duplicate intent evaluation turn id ${turn.id}`);
    }
    ids.add(turn.id);
    if (typeof turn.input !== 'string' || turn.input.trim().length === 0) {
      throw new Error(`intent evaluation turn ${turn.id} requires non-empty input`);
    }
    if (turn.restart_after !== undefined && typeof turn.restart_after !== 'boolean') {
      throw new Error(`intent evaluation turn ${turn.id} restart_after must be boolean`);
    }
  }
  return document;
}

function preparePrompt(prompt, intentOnly = false) {
  const trimmed = prompt.trim();
  if (!trimmed.startsWith('{')) {
    if (intentOnly) {
      throw new Error('intent evaluation requires a schema_version 3 JSON document');
    }
    return { prompt, intent: false };
  }
  const document = JSON.parse(trimmed);
  if (document?.schema_version === 3 || intentOnly) {
    validateIntentDocument(document);
    return { prompt: trimmed, intent: true };
  }
  return { prompt: JSON.stringify(hydrateFixtures(document)), intent: false };
}

function hydratePrompt(prompt) {
  return preparePrompt(prompt).prompt;
}

function bindingDocument(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('intent provider requires a strict bindings object');
  }
  const allowed = value.role_bindings === undefined
    ? ['schema_version', 'channel_bindings']
    : ['schema_version', 'channel_bindings', 'role_bindings'];
  exactKeys(value, allowed, 'intent bindings');
  if (value.schema_version !== 1) {
    throw new Error('intent bindings require schema_version 1');
  }
  if (!Array.isArray(value.channel_bindings) || value.channel_bindings.length === 0) {
    throw new Error('intent bindings require at least one channel binding');
  }
  const roles = value.role_bindings ?? [];
  if (!Array.isArray(roles)) {
    throw new Error('intent role_bindings must be an array');
  }
  const keys = new Set();
  const ids = new Set();
  for (const [kind, entries] of [['channel', value.channel_bindings], ['role', roles]]) {
    for (const [index, entry] of entries.entries()) {
      if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
        throw new Error(`${kind} binding ${index + 1} must be an object`);
      }
      exactKeys(entry, ['key', 'id'], `${kind} binding ${index + 1}`);
      if (typeof entry.key !== 'string'
        || entry.key.length === 0
        || entry.key.length > 64
        || !/^[A-Za-z0-9_.:/-]+$/.test(entry.key)) {
        throw new Error(`${kind} binding ${index + 1} has an invalid key`);
      }
      if (typeof entry.id !== 'string' || !/^[1-9][0-9]*$/.test(entry.id)) {
        throw new Error(`${kind} binding ${index + 1} has an invalid Discord ID`);
      }
      const id = BigInt(entry.id);
      if (id > UINT64_MAX) {
        throw new Error(`${kind} binding ${index + 1} Discord ID exceeds u64`);
      }
      if (keys.has(entry.key)) {
        throw new Error(`duplicate intent binding key ${entry.key}`);
      }
      if (ids.has(entry.id)) {
        throw new Error(`duplicate intent binding Discord ID ${entry.id}`);
      }
      keys.add(entry.key);
      ids.add(entry.id);
    }
  }
  return JSON.stringify({
    schema_version: 1,
    channel_bindings: value.channel_bindings,
    role_bindings: roles,
  });
}

function sourceState(root) {
  const commit = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();
  const status = execFileSync('git', ['status', '--porcelain', '--untracked-files=normal'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();
  if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(commit)) {
    throw new Error('could not determine an exact source commit');
  }
  return { commit, dirty: status.length > 0 };
}

function gatewayIdentity(baseUrl) {
  const gateway = new URL(baseUrl);
  if (!['http:', 'https:'].includes(gateway.protocol) || gateway.username || gateway.password) {
    throw new Error('the model worker URL must identify an HTTP endpoint without credentials');
  }
  const normalized = `${gateway.origin}${gateway.pathname.replace(/\/$/, '')}`;
  return `sha256-${createHash('sha256').update(normalized).digest('hex')}`;
}

function binaryIdentity(binary) {
  return createHash('sha256').update(fs.readFileSync(binary)).digest('hex');
}

function cargoExecutable() {
  if (typeof process.env.CARGO === 'string' && process.env.CARGO.trim().length > 0) {
    return process.env.CARGO.trim();
  }
  return execFileSync('rustup', ['which', 'cargo'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function cargoBuildEnvironment(cargo) {
  const env = { ...process.env };
  if (path.isAbsolute(cargo)) {
    env.PATH = [path.dirname(cargo), process.env.PATH].filter(Boolean).join(path.delimiter);
  }
  return env;
}

class DesignHarnessProvider {
  constructor(options = {}) {
    this.config = options.config || {};
  }

  id() {
    const selected = this.config.intentOnly === true
      ? INTENT_MODEL
      : this.config.model || process.env.STARRING_LLM_MODEL || 'default';
    const model = selected.replace(/[^a-zA-Z0-9_-]/g, '-');
    return `design-harness-${model}`;
  }

  async callApi(prompt) {
    let prepared;
    try {
      prepared = preparePrompt(prompt, this.config.intentOnly === true);
    } catch (error) {
      return { error: `invalid evaluation input: ${error.message}` };
    }
    if (prepared.intent) {
      if (!process.env.STARRING_CODEX_WORKER_URL) {
        return { error: 'STARRING_CODEX_WORKER_URL is required' };
      }
      if (!process.env.STARRING_CODEX_WORKER_TOKEN) {
        return { error: 'STARRING_CODEX_WORKER_TOKEN is required' };
      }
    } else {
      if (!process.env.STARRING_LLM_BASE_URL) {
        return { error: 'STARRING_LLM_BASE_URL is required' };
      }
      if (!process.env.STARRING_LLM_API_KEY) {
        return { error: 'STARRING_LLM_API_KEY is required' };
      }
    }
    const root = path.resolve(__dirname, '..', '..');
    const binaryOverride = process.env.STARRING_HARNESS_BIN;
    if (prepared.intent && binaryOverride && this.config.allowHarnessOverrideForTest !== true) {
      return { error: 'STARRING_HARNESS_BIN is forbidden for intent acceptance runs' };
    }
    if (prepared.intent && this.config.allowHarnessOverrideForTest !== true) {
      try {
        const cargo = cargoExecutable();
        execFileSync(cargo, ['build', '--locked', '-p', 'design-harness-cli'], {
          cwd: root,
          encoding: 'utf8',
          env: cargoBuildEnvironment(cargo),
          stdio: ['ignore', 'pipe', 'pipe'],
        });
      } catch (error) {
        return { error: `intent harness build failed: ${error.message}` };
      }
    }
    const binary = binaryOverride
      ? path.resolve(binaryOverride)
      : path.join(root, 'target', 'debug', 'design-harness-cli');
    const env = { ...process.env };
    if (prepared.intent) {
      try {
        if (this.config.model !== INTENT_MODEL) {
          throw new Error(`intent provider model must be exactly ${INTENT_MODEL}`);
        }
        if (this.config.reasoningEffort !== INTENT_REASONING_EFFORT) {
          throw new Error(
            `intent provider reasoning effort must be exactly ${INTENT_REASONING_EFFORT}`,
          );
        }
        const bindings = bindingDocument(this.config.bindings);
        const source = sourceState(root);
        intentRunOrder += 1;
        env.STARRING_LLM_MODEL = INTENT_MODEL;
        env.STARRING_CODEX_REASONING_EFFORT = INTENT_REASONING_EFFORT;
        env.STARRING_HARNESS_MODE = 'intent_recipe';
        env.STARRING_HARNESS_BINDINGS_JSON = bindings;
        env.STARRING_EVAL_GATEWAY_ID = gatewayIdentity(
          process.env.STARRING_CODEX_WORKER_URL,
        );
        env.STARRING_EVAL_DECLARED_CONTEXT_TOKENS = String(INTENT_CONTEXT_TOKENS);
        env.STARRING_EVAL_SOURCE_COMMIT = source.commit;
        env.STARRING_EVAL_SOURCE_DIRTY = String(source.dirty);
        env.STARRING_EVAL_BINARY_SHA256 = binaryIdentity(binary);
        env.STARRING_EVAL_RUN_ID = intentRunId;
        env.STARRING_EVAL_RUN_ORDER = String(intentRunOrder);
        env.STARRING_HARNESS_MAX_MODEL_CALLS = String(INTENT_SESSION_CONFIG.maxModelCalls);
        env.STARRING_HARNESS_MAX_TOOL_CALLS = String(INTENT_SESSION_CONFIG.maxToolCalls);
        env.STARRING_HARNESS_MAX_GATE_FAILURES = String(INTENT_SESSION_CONFIG.maxGateFailures);
        env.STARRING_HARNESS_CONTEXT_CHARS = String(INTENT_SESSION_CONFIG.contextChars);
        delete env.STARRING_HARNESS_PLANNED;
      } catch (error) {
        return { error: `invalid intent evaluation configuration: ${error.message}` };
      }
    } else if (this.config.model) {
      env.STARRING_LLM_MODEL = this.config.model;
    }

    return new Promise((resolve) => {
      const child = spawn(binary, ['--eval-json'], {
        cwd: root,
        env,
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      let stdout = '';
      let stderr = '';
      let settled = false;
      let terminalError = null;
      let forceKillTimer;
      const environmentTimeoutMs = Number(process.env.STARRING_EVAL_TIMEOUT_MS);
      const configuredTimeoutMs = this.config.timeoutMs
        || (Number.isSafeInteger(environmentTimeoutMs) && environmentTimeoutMs > 0 ? environmentTimeoutMs : 600000);
      const timeoutMs = prepared.intent ? Math.min(configuredTimeoutMs, INTENT_TIMEOUT_MS) : configuredTimeoutMs;
      const maxOutputBytes = this.config.maxOutputBytes || 4194304;
      const finish = (value) => {
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          clearTimeout(forceKillTimer);
          resolve(value);
        }
      };
      const terminate = (message) => {
        if (terminalError) {
          return;
        }
        terminalError = message;
        child.kill('SIGTERM');
        forceKillTimer = setTimeout(() => child.kill('SIGKILL'), this.config.killGraceMs || 2000);
      };
      const timer = setTimeout(() => {
        terminate(`design harness evaluation timed out after ${timeoutMs} milliseconds`);
      }, timeoutMs);
      child.stdout.setEncoding('utf8');
      child.stderr.setEncoding('utf8');
      child.stdout.on('data', (chunk) => {
        stdout += chunk;
        if (Buffer.byteLength(stdout) > maxOutputBytes) {
          terminate(`design harness stdout exceeded ${maxOutputBytes} bytes`);
        }
      });
      child.stderr.on('data', (chunk) => {
        stderr += chunk;
        if (Buffer.byteLength(stderr) > maxOutputBytes) {
          terminate(`design harness stderr exceeded ${maxOutputBytes} bytes`);
        }
      });
      child.on('error', (error) => finish({ error: error.message }));
      child.on('close', (code) => {
        if (settled) {
          return;
        }
        if (terminalError) {
          finish({ error: terminalError });
          return;
        }
        if (code !== 0) {
          finish({ error: stderr.trim() || `design harness exited with code ${code}` });
          return;
        }
        try {
          const report = JSON.parse(stdout.trim());
          finish({ output: JSON.stringify(report), metadata: report });
        } catch (error) {
          finish({ error: `invalid design harness JSON: ${error.message}` });
        }
      });
      child.stdin.on('error', () => {});
      child.stdin.end(prepared.prompt);
    });
  }
}

module.exports = DesignHarnessProvider;
module.exports.binaryIdentity = binaryIdentity;
module.exports.cargoBuildEnvironment = cargoBuildEnvironment;
module.exports.cargoExecutable = cargoExecutable;
module.exports.bindingDocument = bindingDocument;
module.exports.gatewayIdentity = gatewayIdentity;
module.exports.hydratePrompt = hydratePrompt;
module.exports.preparePrompt = preparePrompt;
module.exports.validateIntentDocument = validateIntentDocument;
