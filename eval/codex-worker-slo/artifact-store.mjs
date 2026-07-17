import { createHash, randomUUID } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import {
  link,
  lstat,
  mkdir,
  open,
  readdir,
  unlink,
} from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { assessRun } from "./acceptance.mjs";
import { summarizeRun } from "./summarize.mjs";

const ARTIFACT_FILES = ["raw.json", "summary.json", "acceptance.json"];
const EVIDENCE_FILES = [...ARTIFACT_FILES, "manifest.json"];
const RESERVATIONS = new WeakMap();
const TOOLCHAIN_BASE_KEYS = ["evaluator_revision", "node"];
const TOOLCHAIN_HOST_KEYS = ["evaluator_revision", "machine", "node", "platform"];
const TOOLCHAIN_PLATFORMS = new Set([
  "aix",
  "android",
  "darwin",
  "freebsd",
  "haiku",
  "linux",
  "openbsd",
  "sunos",
  "win32",
]);
const TOOLCHAIN_MACHINES = new Set([
  "aarch64",
  "arm64",
  "loong64",
  "ppc64",
  "ppc64le",
  "riscv64",
  "s390x",
  "x86",
  "x86_64",
]);
const NODE_VERSION_PATTERN = /^v(?:0|[1-9]\d{0,2})\.(?:0|[1-9]\d{0,2})\.(?:0|[1-9]\d{0,3})$/;
const FORBIDDEN_KEYS = new Set([
  "api_key",
  "authorization",
  "bearer_token",
  "cookie",
  "credential",
  "diagnostics",
  "messages",
  "prompt",
  "private_key",
  "request_body",
  "response_body",
  "secret",
  "access_key_id",
  "password",
  "temp_path",
  "token",
  "tool_arguments",
  "tool_schema",
]);

export class ArtifactError extends Error {
  constructor(code) {
    super(code);
    this.name = "ArtifactError";
    this.code = code;
  }
}

function stable(value) {
  if (Array.isArray(value)) {
    return value.map(stable);
  }
  if (value === null || typeof value !== "object") {
    return value;
  }
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
}

function canonical(value) {
  return JSON.stringify(stable(value));
}

function sameDocument(left, right) {
  return canonical(left) === canonical(right);
}

function validatedToolchain(value) {
  if (!value
    || typeof value !== "object"
    || Array.isArray(value)
    || Object.getPrototypeOf(value) !== Object.prototype) {
    throw new ArtifactError("invalid_toolchain");
  }
  const keys = Object.keys(value).sort();
  const baseShape = sameDocument(keys, TOOLCHAIN_BASE_KEYS);
  const hostShape = sameDocument(keys, TOOLCHAIN_HOST_KEYS);
  if (!baseShape && !hostShape) {
    throw new ArtifactError("invalid_toolchain");
  }
  if (value.evaluator_revision !== 1
    || typeof value.node !== "string"
    || !NODE_VERSION_PATTERN.test(value.node)) {
    throw new ArtifactError("invalid_toolchain");
  }
  if (hostShape
    && (!TOOLCHAIN_PLATFORMS.has(value.platform)
      || !TOOLCHAIN_MACHINES.has(value.machine))) {
    throw new ArtifactError("invalid_toolchain");
  }
  return structuredClone(value);
}

export function validateArtifactToolchain(value) {
  return validatedToolchain(value);
}

function validateRunId(runId) {
  if (typeof runId !== "string" || !/^[a-z0-9][a-z0-9_-]{0,127}$/.test(runId)) {
    throw new ArtifactError("invalid_run_id");
  }
}

export function assertSecretFree(value) {
  const visit = (entry) => {
    if (typeof entry === "string") {
      if (/\bBearer\s+[A-Za-z0-9._~-]+/i.test(entry)
        || /\b(?:cfat|cfut)_[A-Za-z0-9_-]+\b/.test(entry)) {
        throw new ArtifactError("secret_material_forbidden");
      }
      return;
    }
    if (Array.isArray(entry)) {
      entry.forEach(visit);
      return;
    }
    if (entry && typeof entry === "object") {
      for (const [key, child] of Object.entries(entry)) {
        const normalizedKey = key.toLowerCase();
        if (FORBIDDEN_KEYS.has(normalizedKey)
          || /(?:^|_)(?:token|secret|api_key)$/.test(normalizedKey)) {
          throw new ArtifactError("secret_field_forbidden");
        }
        visit(child);
      }
    }
  };
  visit(value);
  return value;
}

function serialize(value) {
  assertSecretFree(value);
  return `${JSON.stringify(stable(value), null, 2)}\n`;
}

function digest(content) {
  return createHash("sha256").update(content).digest("hex");
}

async function atomicWrite(directory, name, content) {
  const temporary = join(directory, `.${name}.${randomUUID()}.tmp`);
  const destination = join(directory, name);
  let file = null;
  try {
    file = await open(
      temporary,
      fsConstants.O_WRONLY
        | fsConstants.O_CREAT
        | fsConstants.O_EXCL
        | fsConstants.O_NOFOLLOW,
      0o600,
    );
    await file.chmod(0o600);
    await file.writeFile(content, "utf8");
    await file.sync();
    await file.close();
    file = null;
    await link(temporary, destination);
    await unlink(temporary);
  } catch (error) {
    if (file !== null) {
      await file.close().catch(() => {});
    }
    await unlink(temporary).catch((cleanupError) => {
      if (cleanupError.code !== "ENOENT") {
        throw cleanupError;
      }
    });
    if (error.code === "EEXIST") {
      throw new ArtifactError("artifact_destination_exists");
    }
    throw error;
  }
}

async function secureCreatedDirectory(directory) {
  const listedInfo = await lstat(directory);
  if (!privateDirectory(listedInfo)) {
    throw new ArtifactError("artifact_directory_creation_mode_invalid");
  }
  const directoryHandle = await open(
    directory,
    fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW,
  );
  try {
    const openedInfo = await directoryHandle.stat();
    if (!openedInfo.isDirectory()
      || openedInfo.dev !== listedInfo.dev
      || openedInfo.ino !== listedInfo.ino
      || !privateDirectory(openedInfo)) {
      throw new ArtifactError("artifact_directory_changed_during_creation");
    }
  } finally {
    await directoryHandle.close();
  }
  const privateInfo = await lstat(directory);
  if (!privateInfo.isDirectory()
    || privateInfo.isSymbolicLink()
    || privateInfo.dev !== listedInfo.dev
    || privateInfo.ino !== listedInfo.ino
    || (typeof process.getuid === "function" && privateInfo.uid !== process.getuid())
    || (privateInfo.mode & 0o777) !== 0o700) {
    throw new ArtifactError("artifact_directory_changed_during_creation");
  }
}

function privateDirectory(info) {
  return info.isDirectory()
    && !info.isSymbolicLink()
    && (info.mode & 0o777) === 0o700
    && (typeof process.getuid !== "function" || info.uid === process.getuid());
}

function sameIdentity(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}

async function openPrivateDirectory(directory, errorCode) {
  let listedInfo;
  try {
    listedInfo = await lstat(directory);
  } catch (error) {
    if (error.code === "ENOENT") {
      throw error;
    }
    throw new ArtifactError(errorCode);
  }
  if (!privateDirectory(listedInfo)) {
    throw new ArtifactError(errorCode);
  }
  let handle;
  try {
    handle = await open(
      directory,
      fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW,
    );
    const openedInfo = await handle.stat();
    if (!privateDirectory(openedInfo) || !sameIdentity(openedInfo, listedInfo)) {
      throw new ArtifactError(errorCode);
    }
    return {
      directory,
      handle,
      identity: {
        dev: openedInfo.dev,
        ino: openedInfo.ino,
        uid: openedInfo.uid,
      },
    };
  } catch (error) {
    if (handle !== undefined) {
      await handle.close().catch(() => {});
    }
    if (error instanceof ArtifactError) {
      throw error;
    }
    throw new ArtifactError(errorCode);
  }
}

async function assertDirectoryRecord(record, errorCode) {
  let listedInfo;
  let openedInfo;
  try {
    [listedInfo, openedInfo] = await Promise.all([
      lstat(record.directory),
      record.handle.stat(),
    ]);
  } catch {
    throw new ArtifactError(errorCode);
  }
  if (!privateDirectory(listedInfo)
    || !privateDirectory(openedInfo)
    || !sameIdentity(listedInfo, record.identity)
    || !sameIdentity(openedInfo, record.identity)) {
    throw new ArtifactError(errorCode);
  }
}

async function openRootParent(rootDirectory) {
  const parentDirectory = dirname(rootDirectory);
  if (parentDirectory === rootDirectory) {
    throw new ArtifactError("invalid_artifact_root_parent");
  }
  try {
    return await openPrivateDirectory(parentDirectory, "invalid_artifact_root_parent");
  } catch (error) {
    if (error.code === "ENOENT") {
      throw new ArtifactError("artifact_root_parent_missing");
    }
    throw error;
  }
}

async function prepareRoot(rootDirectory, strictParent) {
  let rootExists;
  try {
    await lstat(rootDirectory);
    rootExists = true;
  } catch (error) {
    if (error.code !== "ENOENT") {
      throw new ArtifactError("invalid_artifact_root");
    }
    rootExists = false;
  }
  let parent = null;
  let root = null;
  try {
    if (strictParent || !rootExists) {
      parent = await openRootParent(rootDirectory);
    }
    if (!rootExists) {
      try {
        await mkdir(rootDirectory, { mode: 0o700 });
        await secureCreatedDirectory(rootDirectory);
      } catch (error) {
        if (error.code !== "EEXIST") {
          throw error;
        }
      }
    }
    root = await openPrivateDirectory(rootDirectory, "invalid_artifact_root");
    if (parent !== null) {
      await assertDirectoryRecord(parent, "artifact_root_parent_changed");
    }
    return { parent, root };
  } catch (error) {
    await root?.handle.close().catch(() => {});
    await parent?.handle.close().catch(() => {});
    throw error;
  }
}

async function closeReservationRecord(record) {
  record.status = "consumed";
  await Promise.allSettled([
    record.run.handle.close(),
    record.root.handle.close(),
    record.parent?.handle.close(),
  ].filter(Boolean));
}

async function assertReservationReady(record) {
  if (record.parent !== null) {
    await assertDirectoryRecord(record.parent, "artifact_root_parent_changed");
  }
  await assertDirectoryRecord(record.root, "artifact_root_changed");
  await assertDirectoryRecord(record.run, "artifact_reservation_changed");
  let names;
  try {
    names = await readdir(record.directory);
  } catch {
    throw new ArtifactError("artifact_reservation_changed");
  }
  if (names.length !== 0) {
    throw new ArtifactError("artifact_reserved_directory_not_empty");
  }
  await assertDirectoryRecord(record.run, "artifact_reservation_changed");
}

async function createReservation(rootDirectory, runId, strictParent) {
  const prepared = await prepareRoot(rootDirectory, strictParent);
  const directory = join(rootDirectory, runId);
  let run = null;
  try {
    try {
      await mkdir(directory, { recursive: false, mode: 0o700 });
    } catch (error) {
      if (error.code === "EEXIST") {
        throw new ArtifactError("artifact_destination_exists");
      }
      throw error;
    }
    await secureCreatedDirectory(directory);
    run = await openPrivateDirectory(directory, "artifact_reservation_changed");
    const record = {
      directory,
      parent: prepared.parent,
      root: prepared.root,
      run,
      runId,
      rootDirectory,
      status: "reserved",
    };
    await assertReservationReady(record);
    const reservation = Object.freeze({ directory, rootDirectory, runId });
    RESERVATIONS.set(reservation, record);
    return reservation;
  } catch (error) {
    await run?.handle.close().catch(() => {});
    await prepared.root.handle.close().catch(() => {});
    await prepared.parent?.handle.close().catch(() => {});
    throw error;
  }
}

function claimReservation(reservation) {
  const record = RESERVATIONS.get(reservation);
  if (record === undefined || record.status !== "reserved") {
    throw new ArtifactError("invalid_artifact_reservation");
  }
  record.status = "claimed";
  return record;
}

export async function reserveEvidenceRun(options) {
  if (!options || typeof options.rootDirectory !== "string") {
    throw new ArtifactError("invalid_artifact_root");
  }
  validateRunId(options.runId);
  return createReservation(resolve(options.rootDirectory), options.runId, true);
}

export async function releaseEvidenceRunReservation(reservation) {
  const record = RESERVATIONS.get(reservation);
  if (record === undefined || record.status !== "reserved") {
    throw new ArtifactError("invalid_artifact_reservation");
  }
  RESERVATIONS.delete(reservation);
  await closeReservationRecord(record);
}

function buildManifest(raw, acceptance, hashes) {
  const toolchain = validatedToolchain(raw.artifact_toolchain);
  return {
    schema_version: 1,
    run_id: raw.run_id,
    source_commit: raw.source.commit,
    source_dirty: raw.source.dirty,
    plan_id: raw.plan.id,
    plan_revision: raw.plan.revision,
    plan_digest: raw.plan_digest,
    worker_instance_id: raw.worker_boundary?.instance_id ?? null,
    worker_source_sha256: raw.worker_boundary?.worker_source_sha256 ?? null,
    worker_identity: raw.worker_boundary?.identity ?? null,
    worker_profile: raw.worker_boundary?.profile ?? null,
    toolchain,
    started_at: raw.started_at,
    completed_at: raw.completed_at,
    start_counters: {
      accepted: raw.counters.start_accepted,
      settled: raw.counters.start_settled,
    },
    end_counters: {
      accepted: raw.counters.end_accepted,
      settled: raw.counters.end_settled,
    },
    acceptance_verdict: acceptance.verdict,
    acceptance_claims: structuredClone(acceptance.claims),
    artifact_hashes: hashes,
  };
}

function recomputeDocuments(raw) {
  try {
    const summary = summarizeRun(structuredClone(raw));
    const acceptance = assessRun(structuredClone(raw.plan), structuredClone(raw), summary);
    return { summary, acceptance };
  } catch {
    throw new ArtifactError("artifact_recomputation_failed");
  }
}

function assertDeterministicDocuments(raw, summary, acceptance) {
  const recomputed = recomputeDocuments(raw);
  if (!sameDocument(summary, recomputed.summary)) {
    throw new ArtifactError("artifact_summary_mismatch");
  }
  if (!sameDocument(acceptance, recomputed.acceptance)) {
    throw new ArtifactError("artifact_acceptance_mismatch");
  }
}

export async function writeEvidenceRun(options) {
  let reservation = options?.reservation ?? null;
  let record = null;
  try {
    if (reservation !== null) {
      record = claimReservation(reservation);
    }
    const raw = structuredClone(options.raw);
    const summary = structuredClone(options.summary);
    const acceptance = structuredClone(options.acceptance);
    validateRunId(raw.run_id);
    if (Object.hasOwn(raw, "artifact_toolchain")) {
      throw new ArtifactError("raw_toolchain_field_reserved");
    }
    raw.artifact_toolchain = validatedToolchain(options.toolchain);
    if (summary.run_id !== raw.run_id || acceptance.run_id !== raw.run_id) {
      throw new ArtifactError("artifact_run_id_mismatch");
    }
    if (summary.plan_id !== raw.plan.id || acceptance.plan_id !== raw.plan.id) {
      throw new ArtifactError("artifact_plan_mismatch");
    }
    assertDeterministicDocuments(raw, summary, acceptance);
    const documents = {
      "raw.json": serialize(raw),
      "summary.json": serialize(summary),
      "acceptance.json": serialize(acceptance),
    };
    const hashes = Object.fromEntries(
      ARTIFACT_FILES.map((name) => [name, digest(documents[name])]),
    );
    const manifest = buildManifest(raw, acceptance, hashes);
    const manifestContent = serialize(manifest);
    let rootDirectory;
    if (record !== null) {
      rootDirectory = record.rootDirectory;
      if (options.rootDirectory !== undefined
        && (typeof options.rootDirectory !== "string"
          || resolve(options.rootDirectory) !== rootDirectory)) {
        throw new ArtifactError("artifact_reservation_mismatch");
      }
      if (record.runId !== raw.run_id) {
        throw new ArtifactError("artifact_reservation_mismatch");
      }
    } else {
      if (typeof options.rootDirectory !== "string") {
        throw new ArtifactError("invalid_artifact_root");
      }
      rootDirectory = resolve(options.rootDirectory);
      reservation = await createReservation(rootDirectory, raw.run_id, false);
      record = claimReservation(reservation);
    }
    await assertReservationReady(record);
    for (const name of ARTIFACT_FILES) {
      await atomicWrite(record.directory, name, documents[name]);
      await assertDirectoryRecord(record.run, "artifact_reservation_changed");
    }
    await atomicWrite(record.directory, "manifest.json", manifestContent);
    await assertDirectoryRecord(record.run, "artifact_reservation_changed");
    const names = (await readdir(record.directory)).sort();
    if (!sameDocument(names, [...EVIDENCE_FILES].sort())) {
      throw new ArtifactError("artifact_file_set_mismatch");
    }
    await record.run.handle.sync();
    await assertDirectoryRecord(record.run, "artifact_reservation_changed");
    if (record.parent !== null) {
      await assertDirectoryRecord(record.parent, "artifact_root_parent_changed");
    }
    await assertDirectoryRecord(record.root, "artifact_root_changed");
    return {
      directory: record.directory,
      manifest,
    };
  } finally {
    if (record !== null) {
      RESERVATIONS.delete(reservation);
      await closeReservationRecord(record);
    }
  }
}

async function readDocument(directory, name) {
  const path = join(directory, name);
  const listedInfo = await lstat(path);
  if (listedInfo.isSymbolicLink()) {
    throw new ArtifactError("artifact_file_symlink_forbidden");
  }
  if (!listedInfo.isFile()) {
    throw new ArtifactError("artifact_file_not_regular");
  }
  if ((listedInfo.mode & 0o777) !== 0o600) {
    throw new ArtifactError("artifact_file_not_private");
  }
  let file;
  try {
    file = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  } catch (error) {
    if (error.code === "ELOOP") {
      throw new ArtifactError("artifact_file_symlink_forbidden");
    }
    throw error;
  }
  let content;
  try {
    const openedInfo = await file.stat();
    if (!openedInfo.isFile()
      || openedInfo.dev !== listedInfo.dev
      || openedInfo.ino !== listedInfo.ino) {
      throw new ArtifactError("artifact_file_changed_during_read");
    }
    if ((openedInfo.mode & 0o777) !== 0o600) {
      throw new ArtifactError("artifact_file_not_private");
    }
    content = await file.readFile("utf8");
  } finally {
    await file.close();
  }
  let document;
  try {
    document = JSON.parse(content);
  } catch {
    throw new ArtifactError("artifact_invalid_json");
  }
  assertSecretFree(document);
  return { content, document };
}

export async function verifyEvidenceRun(directoryInput) {
  const directory = resolve(directoryInput);
  const directoryInfo = await lstat(directory);
  if (!directoryInfo.isDirectory()
    || directoryInfo.isSymbolicLink()
    || (directoryInfo.mode & 0o777) !== 0o700) {
    throw new ArtifactError("artifact_directory_not_private");
  }
  const names = (await readdir(directory)).sort();
  if (!sameDocument(names, [...EVIDENCE_FILES].sort())) {
    throw new ArtifactError("artifact_file_set_mismatch");
  }
  const documents = {};
  for (const name of EVIDENCE_FILES) {
    documents[name] = await readDocument(directory, name);
  }
  const manifest = documents["manifest.json"].document;
  if (Object.hasOwn(manifest.artifact_hashes ?? {}, "manifest.json")) {
    throw new ArtifactError("manifest_self_hash_forbidden");
  }
  const hashNames = Object.keys(manifest.artifact_hashes ?? {}).sort();
  if (!sameDocument(hashNames, [...ARTIFACT_FILES].sort())) {
    throw new ArtifactError("manifest_hash_set_mismatch");
  }
  const actualHashes = {};
  for (const name of ARTIFACT_FILES) {
    actualHashes[name] = digest(documents[name].content);
    if (manifest.artifact_hashes[name] !== actualHashes[name]) {
      throw new ArtifactError("artifact_hash_mismatch");
    }
  }
  const runIds = ARTIFACT_FILES.map((name) => documents[name].document.run_id);
  if (runIds.some((runId) => runId !== manifest.run_id)) {
    throw new ArtifactError("artifact_run_id_mismatch");
  }
  const raw = documents["raw.json"].document;
  const summary = documents["summary.json"].document;
  const acceptance = documents["acceptance.json"].document;
  validatedToolchain(manifest.toolchain);
  assertDeterministicDocuments(raw, summary, acceptance);
  const expectedManifest = buildManifest(raw, acceptance, actualHashes);
  if (!sameDocument(manifest, expectedManifest)) {
    throw new ArtifactError("manifest_boundary_mismatch");
  }
  return {
    raw,
    summary,
    acceptance,
    manifest,
  };
}
