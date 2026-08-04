import assert from "node:assert/strict";
import {
  appendFile,
  chmod,
  lstat,
  mkdtemp,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  MetricsReaderError,
  createFileMetricsReader,
  validateMetricsFileIdentity,
} from "./metrics-reader.mjs";

function metric(requestId, instanceId = "instance-1") {
  return {
    metric_schema_version: 3,
    timestamp: `2026-07-17T00:00:0${requestId.at(-1)}.000Z`,
    request_id: requestId,
    instance_id: instanceId,
    worker_source_sha256: "a".repeat(64),
    completion_sha256: "c".repeat(64),
  };
}

function context(expectedRecords = 2) {
  return {
    worker_boundary: {
      instance_id: "instance-1",
      worker_source_sha256: "a".repeat(64),
    },
    expected_records: expectedRecords,
  };
}

test("file metrics reader returns only post-boundary records for one worker", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-reader-"));
  const path = join(directory, "worker.jsonl");
  try {
    await writeFile(path, `${JSON.stringify(metric("request-1"))}\n`, { mode: 0o600 });
    await chmod(path, 0o600);
    const reader = await createFileMetricsReader({ path, backups: 1 });
    await appendFile(path, `${JSON.stringify(metric("request-2"))}\n`, "utf8");
    await appendFile(path, `${JSON.stringify(metric("request-3"))}\n`, "utf8");
    await appendFile(path, `${JSON.stringify(metric("request-4", "other"))}\n`, "utf8");
    const records = await reader(context());
    assert.deepEqual(records.map((record) => record.request_id), ["request-2", "request-3"]);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("file metrics reader rejects symbolic links and malformed JSONL", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-safety-"));
  try {
    const target = join(directory, "target.jsonl");
    const link = join(directory, "link.jsonl");
    await writeFile(target, "{}\n", { mode: 0o600 });
    await symlink(target, link);
    await assert.rejects(
      createFileMetricsReader({ path: link }),
      (error) => error instanceof MetricsReaderError && error.code === "unsafe_metrics_file",
    );
    const malformed = join(directory, "malformed.jsonl");
    await writeFile(malformed, "not-json\n", { mode: 0o600 });
    await assert.rejects(
      createFileMetricsReader({ path: malformed }),
      (error) => error instanceof MetricsReaderError && error.code === "metrics_invalid_jsonl",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("file metrics reader validates opened file identity", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-identity-"));
  try {
    const first = join(directory, "first.jsonl");
    const replacement = join(directory, "replacement.jsonl");
    await writeFile(first, "{}\n", { mode: 0o600 });
    await writeFile(replacement, "{}\n", { mode: 0o600 });
    await chmod(first, 0o600);
    await chmod(replacement, 0o600);
    const listed = await lstat(first, { bigint: true });
    const same = await lstat(first, { bigint: true });
    const changed = await lstat(replacement, { bigint: true });
    assert.doesNotThrow(() => validateMetricsFileIdentity(listed, same));
    assert.throws(
      () => validateMetricsFileIdentity(listed, changed),
      (error) => error instanceof MetricsReaderError && error.code === "metrics_file_changed",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("file metrics reader rejects files that grow beyond the bound", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-growth-"));
  const path = join(directory, "worker.jsonl");
  try {
    await writeFile(path, `${JSON.stringify(metric("request-1"))}\n`, { mode: 0o600 });
    await chmod(path, 0o600);
    const reader = await createFileMetricsReader({ path, backups: 0 });
    await appendFile(path, Buffer.alloc(8_000_000));
    await assert.rejects(
      reader(context()),
      (error) => error instanceof MetricsReaderError && error.code === "unsafe_metrics_file",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("file metrics reader enforces the bound for rotated files", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-rotated-"));
  const path = join(directory, "worker.jsonl");
  try {
    await writeFile(`${path}.1`, Buffer.alloc(8_000_001), { mode: 0o600 });
    await chmod(`${path}.1`, 0o600);
    await assert.rejects(
      createFileMetricsReader({ path, backups: 1 }),
      (error) => error instanceof MetricsReaderError && error.code === "unsafe_metrics_file",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("file metrics reader rejects non-private file modes", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-mode-"));
  const path = join(directory, "worker.jsonl");
  try {
    await writeFile(path, "{}\n", { mode: 0o600 });
    await chmod(path, 0o640);
    await assert.rejects(
      createFileMetricsReader({ path, backups: 0 }),
      (error) => error instanceof MetricsReaderError && error.code === "unsafe_metrics_file",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("file metrics reader stops with a bounded abort code", async () => {
  const directory = await mkdtemp(join(tmpdir(), "starring-slo-metrics-abort-"));
  const path = join(directory, "worker.jsonl");
  try {
    await writeFile(path, `${JSON.stringify(metric("request-1"))}\n`, { mode: 0o600 });
    await chmod(path, 0o600);
    const reader = await createFileMetricsReader({ path, backups: 0 });
    const controller = new AbortController();
    controller.abort();
    await assert.rejects(
      reader({ ...context(), signal: controller.signal }),
      (error) => error instanceof MetricsReaderError && error.code === "metrics_read_aborted",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
