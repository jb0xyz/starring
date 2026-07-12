const { spawn } = require('node:child_process');
const path = require('node:path');

class DesignHarnessProvider {
  constructor(options = {}) {
    this.config = options.config || {};
  }

  id() {
    const model = (this.config.model || 'default').replace(/[^a-zA-Z0-9_-]/g, '-');
    return `design-harness-${model}`;
  }

  async callApi(prompt) {
    if (!process.env.STARRING_LLM_BASE_URL) {
      return { error: 'STARRING_LLM_BASE_URL is required' };
    }
    if (!process.env.STARRING_LLM_API_KEY) {
      return { error: 'STARRING_LLM_API_KEY is required' };
    }
    const root = path.resolve(__dirname, '..', '..');
    const binary = process.env.STARRING_HARNESS_BIN
      ? path.resolve(process.env.STARRING_HARNESS_BIN)
      : path.join(root, 'target', 'debug', 'design-harness-cli');
    const env = { ...process.env };
    if (this.config.model) {
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
      const timeoutMs = this.config.timeoutMs || 600000;
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
      child.stdin.end(prompt);
    });
  }
}

module.exports = DesignHarnessProvider;
