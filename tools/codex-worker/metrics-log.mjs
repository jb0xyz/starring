import { appendFile, chmod, mkdir, open, rename, rm, stat } from "node:fs/promises";
import { dirname } from "node:path";

async function fileSize(path) {
  try {
    return (await stat(path)).size;
  } catch (error) {
    if (error?.code === "ENOENT") {
      return 0;
    }
    throw error;
  }
}

async function moveIfPresent(source, destination) {
  try {
    await rename(source, destination);
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
}

export class MetricsLog {
  constructor(path, options = {}) {
    this.path = path;
    this.maxBytes = options.maxBytes ?? 5_000_000;
    this.backups = options.backups ?? 3;
    this.pending = Promise.resolve();
    this.lastError = null;
    this.writableVerified = false;
    this.recordsAttempted = 0;
    this.recordsWritten = 0;
    this.pendingRecords = 0;
    this.writeFailures = 0;
  }

  record(value) {
    const line = `${JSON.stringify(value)}\n`;
    this.recordsAttempted += 1;
    this.pendingRecords += 1;
    this.pending = this.pending
      .catch(() => {})
      .then(async () => {
        await this.write(line);
        this.recordsWritten += 1;
      })
      .catch((error) => {
        this.lastError = error;
        this.writeFailures += 1;
      })
      .finally(() => {
        this.pendingRecords -= 1;
      });
    return this.pending;
  }

  async verifyWritable() {
    await this.ensureDestination();
    const handle = await open(this.path, "a", 0o600);
    await handle.close();
    await chmod(this.path, 0o600);
    this.writableVerified = true;
  }

  async ensureDestination() {
    const directory = dirname(this.path);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await chmod(directory, 0o700);
  }

  async write(line) {
    await this.ensureDestination();
    if ((await fileSize(this.path)) + Buffer.byteLength(line) > this.maxBytes) {
      await this.rotate();
    }
    await appendFile(this.path, line, { encoding: "utf8", mode: 0o600 });
    await chmod(this.path, 0o600);
  }

  async rotate() {
    for (let index = this.backups; index >= 1; index -= 1) {
      const source = index === 1 ? this.path : `${this.path}.${index - 1}`;
      const destination = `${this.path}.${index}`;
      await rm(destination, { force: true });
      await moveIfPresent(source, destination);
    }
  }

  async flush() {
    await this.pending;
  }

  snapshot() {
    return {
      status: this.writableVerified && this.writeFailures === 0 ? "ok" : "degraded",
      writable_verified: this.writableVerified,
      records_attempted: this.recordsAttempted,
      records_written: this.recordsWritten,
      pending_records: this.pendingRecords,
      write_failures_total: this.writeFailures,
      last_error_code: this.lastError === null ? null : "metrics_write_failed",
    };
  }
}
