import { constants } from "node:fs";
import { lstat, open } from "node:fs/promises";
import { resolve } from "node:path";

const MAX_FILE_BYTES = 8_000_000;
const MAX_LINE_BYTES = 1_000_000;
const READ_CHUNK_BYTES = 65_536;

export class MetricsReaderError extends Error {
  constructor(code) {
    super(code);
    this.name = "MetricsReaderError";
    this.code = code;
  }
}

function throwIfAborted(signal) {
  if (signal?.aborted) {
    throw new MetricsReaderError("metrics_read_aborted");
  }
}

function ownerUid() {
  if (typeof process.getuid !== "function") {
    throw new MetricsReaderError("metrics_owner_unavailable");
  }
  return BigInt(process.getuid());
}

function integer(value) {
  return typeof value === "bigint" ? value : BigInt(value);
}

function validateMetadata(metadata) {
  if (!metadata.isFile()
    || metadata.isSymbolicLink()
    || (integer(metadata.mode) & 0o077n) !== 0n
    || integer(metadata.uid) !== ownerUid()
    || integer(metadata.size) > BigInt(MAX_FILE_BYTES)) {
    throw new MetricsReaderError("unsafe_metrics_file");
  }
}

export function validateMetricsFileIdentity(listed, opened) {
  validateMetadata(listed);
  validateMetadata(opened);
  if (integer(listed.dev) !== integer(opened.dev)
    || integer(listed.ino) !== integer(opened.ino)) {
    throw new MetricsReaderError("metrics_file_changed");
  }
}

function normalizeReadError(error, signal) {
  if (error instanceof MetricsReaderError) {
    return error;
  }
  if (signal?.aborted) {
    return new MetricsReaderError("metrics_read_aborted");
  }
  if (error?.code === "ELOOP") {
    return new MetricsReaderError("unsafe_metrics_file");
  }
  return new MetricsReaderError("metrics_read_failed");
}

async function readListedMetadata(path, signal, allowAbsent) {
  throwIfAborted(signal);
  try {
    const metadata = await lstat(path, { bigint: true });
    throwIfAborted(signal);
    return metadata;
  } catch (error) {
    if (error instanceof MetricsReaderError) {
      throw error;
    }
    if (allowAbsent && error?.code === "ENOENT") {
      return null;
    }
    if (signal?.aborted) {
      throw new MetricsReaderError("metrics_read_aborted");
    }
    if (error?.code === "ENOENT") {
      throw new MetricsReaderError("metrics_file_changed");
    }
    throw new MetricsReaderError("metrics_read_failed");
  }
}

async function readBounded(handle, signal) {
  const buffer = Buffer.allocUnsafe(MAX_FILE_BYTES + 1);
  let bytesRead = 0;
  while (bytesRead < buffer.length) {
    throwIfAborted(signal);
    const length = Math.min(READ_CHUNK_BYTES, buffer.length - bytesRead);
    const result = await handle.read(buffer, bytesRead, length, bytesRead);
    throwIfAborted(signal);
    if (result.bytesRead === 0) {
      break;
    }
    bytesRead += result.bytesRead;
  }
  if (bytesRead > MAX_FILE_BYTES) {
    throw new MetricsReaderError("unsafe_metrics_file");
  }
  return {
    bytesRead,
    content: buffer.toString("utf8", 0, bytesRead),
  };
}

async function readFileIfPresent(path, signal) {
  const listed = await readListedMetadata(path, signal, true);
  if (listed === null) {
    return "";
  }
  validateMetadata(listed);
  let handle;
  try {
    throwIfAborted(signal);
    handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    throwIfAborted(signal);
    const opened = await handle.stat({ bigint: true });
    validateMetricsFileIdentity(listed, opened);
    const result = await readBounded(handle, signal);
    const finalOpened = await handle.stat({ bigint: true });
    validateMetricsFileIdentity(opened, finalOpened);
    if (integer(finalOpened.size) !== BigInt(result.bytesRead)) {
      throw new MetricsReaderError("metrics_file_changed");
    }
    const finalListed = await readListedMetadata(path, signal, false);
    validateMetricsFileIdentity(finalOpened, finalListed);
    if (integer(finalListed.size) !== integer(finalOpened.size)) {
      throw new MetricsReaderError("metrics_file_changed");
    }
    throwIfAborted(signal);
    return result.content;
  } catch (error) {
    throw normalizeReadError(error, signal);
  } finally {
    try {
      await handle?.close();
    } catch {
    }
  }
}

function parseRecords(content) {
  const records = [];
  for (const line of content.split("\n")) {
    if (line.length === 0) {
      continue;
    }
    if (Buffer.byteLength(line) > MAX_LINE_BYTES) {
      throw new MetricsReaderError("metrics_line_too_large");
    }
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      throw new MetricsReaderError("metrics_invalid_jsonl");
    }
    if (!record || typeof record !== "object" || Array.isArray(record)) {
      throw new MetricsReaderError("metrics_invalid_record");
    }
    records.push(record);
  }
  return records;
}

async function readRecords(path, backups, signal) {
  const paths = [];
  for (let index = backups; index >= 1; index -= 1) {
    paths.push(`${path}.${index}`);
  }
  paths.push(path);
  const records = [];
  for (const candidate of paths) {
    throwIfAborted(signal);
    const parsed = parseRecords(await readFileIfPresent(candidate, signal));
    for (const record of parsed) {
      records.push(record);
    }
  }
  return records;
}

function recordId(record) {
  return typeof record.request_id === "string" && record.request_id.length > 0
    ? record.request_id
    : null;
}

function validSignal(signal) {
  return signal === undefined
    || (signal !== null && typeof signal === "object" && typeof signal.aborted === "boolean");
}

export async function createFileMetricsReader(options) {
  if (!options || typeof options.path !== "string" || options.path.length === 0) {
    throw new MetricsReaderError("invalid_metrics_path");
  }
  if (!validSignal(options.signal)) {
    throw new MetricsReaderError("invalid_metrics_signal");
  }
  const path = resolve(options.path);
  const backups = options.backups ?? 3;
  if (!Number.isSafeInteger(backups) || backups < 0 || backups > 32) {
    throw new MetricsReaderError("invalid_metrics_backups");
  }
  const baseline = new Set(
    (await readRecords(path, backups, options.signal)).map(recordId).filter(Boolean),
  );
  return async (context) => {
    if (!context?.worker_boundary
      || typeof context.worker_boundary.instance_id !== "string"
      || typeof context.worker_boundary.worker_source_sha256 !== "string"
      || !Number.isSafeInteger(context.expected_records)
      || context.expected_records < 0
      || !validSignal(context.signal)) {
      throw new MetricsReaderError("invalid_metrics_context");
    }
    const records = (await readRecords(path, backups, context.signal)).filter((record) => (
      record.metric_schema_version === 3
        && !baseline.has(recordId(record))
        && record.instance_id === context.worker_boundary.instance_id
        && record.worker_source_sha256 === context.worker_boundary.worker_source_sha256
    ));
    records.sort((left, right) => {
      const timestampOrder = String(left.timestamp).localeCompare(String(right.timestamp));
      return timestampOrder === 0
        ? String(left.request_id).localeCompare(String(right.request_id))
        : timestampOrder;
    });
    return records;
  };
}
