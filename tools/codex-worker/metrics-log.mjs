import { appendFile, mkdir, rename, rm, stat } from "node:fs/promises";
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
  }

  record(value) {
    const line = `${JSON.stringify(value)}\n`;
    this.pending = this.pending
      .catch(() => {})
      .then(() => this.write(line))
      .catch((error) => {
        this.lastError = error;
      });
    return this.pending;
  }

  async write(line) {
    await mkdir(dirname(this.path), { recursive: true, mode: 0o700 });
    if ((await fileSize(this.path)) + Buffer.byteLength(line) > this.maxBytes) {
      await this.rotate();
    }
    await appendFile(this.path, line, { encoding: "utf8", mode: 0o600 });
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
}
