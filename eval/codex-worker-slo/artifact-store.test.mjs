import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  stat,
  symlink,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { assessRun } from "./acceptance.mjs";
import {
  ArtifactError,
  assertSecretFree,
  releaseEvidenceRunReservation,
  reserveEvidenceRun,
  verifyEvidenceRun,
  writeEvidenceRun,
} from "./artifact-store.mjs";
import { EXPECTED_IDENTITY, getPlan, planDigest } from "./plans.mjs";
import { summarizeRun } from "./summarize.mjs";

const TOOLCHAIN = { node: process.version, evaluator_revision: 1 };

function evidence() {
  const plan = getPlan("development");
  const raw = {
    schema_version: 1,
    run_id: "artifact-test",
    plan,
    plan_digest: planDigest(plan),
    source: { commit: "a".repeat(40), dirty: false },
    execution_mode: "fake_only",
    started_at: "2026-07-17T00:00:00.000Z",
    completed_at: "2026-07-17T00:00:01.000Z",
    duration_ms: 1_000,
    interrupted: false,
    stop_reason: null,
    automatic_retries: 0,
    planned_live_calls: 0,
    observed_live_calls: 0,
    live_call_count_known: true,
    usage: {
      input_tokens: 0,
      cached_input_tokens: 0,
      output_tokens: 0,
      reasoning_output_tokens: 0,
    },
    worker_boundary: {
      instance_id: "fake-instance",
      worker_source_sha256: "b".repeat(64),
      identity: structuredClone(EXPECTED_IDENTITY),
      profile: structuredClone(plan.worker_profile),
    },
    counters: {
      start_accepted: 0,
      start_settled: 0,
      end_accepted: 12,
      end_settled: 12,
    },
    health_samples: [],
    resource_samples: [],
    resource_errors: [],
    resource_duration_ms: 0,
    worker_metrics: [],
    scenarios: [],
    waves: [],
    observations: [],
  };
  const summary = summarizeRun(raw);
  const acceptance = assessRun(plan, raw, summary);
  return { raw, summary, acceptance };
}

function digest(content) {
  return createHash("sha256").update(content).digest("hex");
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

async function writePrivateJson(path, value) {
  const content = `${JSON.stringify(value, null, 2)}\n`;
  await writeFile(path, content, "utf8");
  await chmod(path, 0o600);
  return content;
}

async function replaceDocument(directory, name, document, syncAcceptance = false) {
  const content = await writePrivateJson(join(directory, name), document);
  const manifestPath = join(directory, "manifest.json");
  const manifest = await readJson(manifestPath);
  manifest.artifact_hashes[name] = digest(content);
  if (syncAcceptance) {
    manifest.acceptance_verdict = document.verdict;
    manifest.acceptance_claims = structuredClone(document.claims);
  }
  await writePrivateJson(manifestPath, manifest);
}

async function writeValidRun(rootDirectory) {
  return writeEvidenceRun({
    rootDirectory,
    ...evidence(),
    toolchain: TOOLCHAIN,
  });
}

function isArtifactError(code) {
  return (error) => error instanceof ArtifactError && error.code === code;
}

test("evidence files are private, exact, hashed, derived, and self-hash free", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const written = await writeValidRun(root);
    assert.equal((await stat(written.directory)).mode & 0o777, 0o700);
    const names = (await readdir(written.directory)).sort();
    assert.deepEqual(names, [
      "acceptance.json",
      "manifest.json",
      "raw.json",
      "summary.json",
    ]);
    for (const name of names) {
      assert.equal((await stat(join(written.directory, name))).mode & 0o777, 0o600);
    }
    assert.equal(Object.hasOwn(written.manifest.artifact_hashes, "manifest.json"), false);
    const verified = await verifyEvidenceRun(written.directory);
    assert.equal(verified.raw.run_id, "artifact-test");
    assert.equal(verified.manifest.source_dirty, false);
    assert.equal(verified.manifest.acceptance_verdict, verified.acceptance.verdict);
    assert.deepEqual(verified.manifest.acceptance_claims, verified.acceptance.claims);
    assert.deepEqual(verified.raw.artifact_toolchain, TOOLCHAIN);
    assert.deepEqual(verified.manifest.toolchain, verified.raw.artifact_toolchain);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("reservation creates and holds one exact private empty run before execution", async () => {
  const parent = await mkdtemp(join(tmpdir(), "starring-slo-parent-"));
  try {
    const root = join(parent, "evidence");
    const reservation = await reserveEvidenceRun({
      rootDirectory: root,
      runId: "artifact-test",
    });
    assert.equal((await stat(root)).mode & 0o777, 0o700);
    assert.equal((await stat(reservation.directory)).mode & 0o777, 0o700);
    assert.deepEqual(await readdir(reservation.directory), []);
    const written = await writeEvidenceRun({
      reservation,
      ...evidence(),
      toolchain: TOOLCHAIN,
    });
    assert.equal(written.directory, reservation.directory);
    await verifyEvidenceRun(written.directory);
    await assert.rejects(
      reserveEvidenceRun({ rootDirectory: root, runId: "artifact-test" }),
      isArtifactError("artifact_destination_exists"),
    );
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});

test("reservation fails before execution when the root parent is missing or unsafe", async () => {
  const parent = await mkdtemp(join(tmpdir(), "starring-slo-parent-"));
  try {
    await assert.rejects(
      reserveEvidenceRun({
        rootDirectory: join(parent, "missing", "evidence"),
        runId: "artifact-test",
      }),
      isArtifactError("artifact_root_parent_missing"),
    );
    const sharedParent = join(parent, "shared-parent");
    await mkdir(sharedParent, { mode: 0o755 });
    await chmod(sharedParent, 0o755);
    await assert.rejects(
      reserveEvidenceRun({
        rootDirectory: join(sharedParent, "evidence"),
        runId: "artifact-test",
      }),
      isArtifactError("invalid_artifact_root_parent"),
    );
    assert.equal((await stat(sharedParent)).mode & 0o777, 0o755);
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});

test("reservation never repairs an existing root and can be released unused", async () => {
  const parent = await mkdtemp(join(tmpdir(), "starring-slo-parent-"));
  try {
    const sharedRoot = join(parent, "shared-root");
    await mkdir(sharedRoot, { mode: 0o755 });
    await chmod(sharedRoot, 0o755);
    await assert.rejects(
      reserveEvidenceRun({ rootDirectory: sharedRoot, runId: "artifact-test" }),
      isArtifactError("invalid_artifact_root"),
    );
    assert.equal((await stat(sharedRoot)).mode & 0o777, 0o755);

    const privateRoot = join(parent, "private-root");
    await mkdir(privateRoot, { mode: 0o700 });
    const rootBefore = await stat(privateRoot);
    const reservation = await reserveEvidenceRun({
      rootDirectory: privateRoot,
      runId: "unused-run",
    });
    const rootAfter = await stat(privateRoot);
    assert.equal(rootAfter.ino, rootBefore.ino);
    assert.equal(rootAfter.mode & 0o777, 0o700);
    await releaseEvidenceRunReservation(reservation);
    await assert.rejects(
      releaseEvidenceRunReservation(reservation),
      isArtifactError("invalid_artifact_reservation"),
    );
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});

test("reserved directory replacement is detected before evidence writes", async () => {
  const parent = await mkdtemp(join(tmpdir(), "starring-slo-parent-"));
  try {
    const root = join(parent, "evidence");
    const reservation = await reserveEvidenceRun({
      rootDirectory: root,
      runId: "artifact-test",
    });
    const replacement = join(parent, "replacement");
    await mkdir(replacement, { mode: 0o700 });
    await rm(reservation.directory, { recursive: true });
    await symlink(replacement, reservation.directory, "dir");
    await assert.rejects(
      writeEvidenceRun({
        reservation,
        ...evidence(),
        toolchain: TOOLCHAIN,
      }),
      isArtifactError("artifact_reservation_changed"),
    );
    assert.deepEqual(await readdir(replacement), []);
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});

test("writer rejects supplied summary and acceptance that are not deterministic", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const summaryMismatch = evidence();
    summaryMismatch.summary.duration_ms += 1;
    await assert.rejects(
      writeEvidenceRun({
        rootDirectory: root,
        ...summaryMismatch,
        toolchain: TOOLCHAIN,
      }),
      isArtifactError("artifact_summary_mismatch"),
    );
    const acceptanceMismatch = evidence();
    acceptanceMismatch.acceptance.claims.diagnostic_complete = !acceptanceMismatch.acceptance
      .claims.diagnostic_complete;
    await assert.rejects(
      writeEvidenceRun({
        rootDirectory: root,
        ...acceptanceMismatch,
        toolchain: TOOLCHAIN,
      }),
      isArtifactError("artifact_acceptance_mismatch"),
    );
    assert.deepEqual(await readdir(root), []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verification rejects a rehashed tampered summary", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const written = await writeValidRun(root);
    const summary = await readJson(join(written.directory, "summary.json"));
    summary.duration_ms += 1;
    await replaceDocument(written.directory, "summary.json", summary);
    await assert.rejects(
      verifyEvidenceRun(written.directory),
      isArtifactError("artifact_summary_mismatch"),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verification rejects rehashed acceptance with synchronized manifest claims", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const written = await writeValidRun(root);
    const acceptance = await readJson(join(written.directory, "acceptance.json"));
    acceptance.claims.diagnostic_complete = !acceptance.claims.diagnostic_complete;
    await replaceDocument(written.directory, "acceptance.json", acceptance, true);
    await assert.rejects(
      verifyEvidenceRun(written.directory),
      isArtifactError("artifact_acceptance_mismatch"),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verification rejects manifest boundary tampering", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const written = await writeValidRun(root);
    const manifestPath = join(written.directory, "manifest.json");
    const manifest = await readJson(manifestPath);
    manifest.worker_instance_id = "tampered-instance";
    await writePrivateJson(manifestPath, manifest);
    await assert.rejects(
      verifyEvidenceRun(written.directory),
      isArtifactError("manifest_boundary_mismatch"),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verification binds manifest toolchain to the hashed raw document", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const written = await writeValidRun(root);
    const manifestPath = join(written.directory, "manifest.json");
    const manifest = await readJson(manifestPath);
    manifest.toolchain.node = "v1.2.3";
    await writePrivateJson(manifestPath, manifest);
    await assert.rejects(
      verifyEvidenceRun(written.directory),
      isArtifactError("manifest_boundary_mismatch"),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verification rejects a rehashed raw toolchain change without manifest agreement", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const written = await writeValidRun(root);
    const raw = await readJson(join(written.directory, "raw.json"));
    raw.artifact_toolchain.node = "v1.2.3";
    await replaceDocument(written.directory, "raw.json", raw);
    await assert.rejects(
      verifyEvidenceRun(written.directory),
      isArtifactError("manifest_boundary_mismatch"),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("writer accepts only exact bounded toolchain schemas", async () => {
  const root = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    for (const toolchain of [
      { ...TOOLCHAIN, credential: "hidden" },
      { node: "secret-token", evaluator_revision: 1 },
      { node: process.version, evaluator_revision: 2 },
      { ...TOOLCHAIN, platform: "darwin" },
      {
        ...TOOLCHAIN,
        platform: "darwin",
        machine: "arm64\ncredential",
      },
    ]) {
      await assert.rejects(
        writeEvidenceRun({
          rootDirectory: root,
          ...evidence(),
          toolchain,
        }),
        isArtifactError("invalid_toolchain"),
      );
    }
    assert.deepEqual(await readdir(root), []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verification rejects symlink artifacts and additional files", async () => {
  const symlinkRoot = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  const extraRoot = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  try {
    const symlinkRun = await writeValidRun(symlinkRoot);
    const target = join(symlinkRoot, "raw-target.json");
    await writeFile(target, "{}\n", { mode: 0o600 });
    await unlink(join(symlinkRun.directory, "raw.json"));
    await symlink(target, join(symlinkRun.directory, "raw.json"));
    await assert.rejects(
      verifyEvidenceRun(symlinkRun.directory),
      isArtifactError("artifact_file_symlink_forbidden"),
    );

    const extraRun = await writeValidRun(extraRoot);
    await writeFile(join(extraRun.directory, ".summary.json.leftover.tmp"), "x", {
      mode: 0o600,
    });
    await assert.rejects(
      verifyEvidenceRun(extraRun.directory),
      isArtifactError("artifact_file_set_mismatch"),
    );
  } finally {
    await rm(symlinkRoot, { recursive: true, force: true });
    await rm(extraRoot, { recursive: true, force: true });
  }
});

test("writer never repairs shared roots and creates new roots privately", async () => {
  const sharedRoot = await mkdtemp(join(tmpdir(), "starring-slo-artifacts-"));
  const parent = await mkdtemp(join(tmpdir(), "starring-slo-parent-"));
  try {
    await chmod(sharedRoot, 0o755);
    await assert.rejects(
      writeValidRun(sharedRoot),
      isArtifactError("invalid_artifact_root"),
    );
    assert.equal((await stat(sharedRoot)).mode & 0o777, 0o755);
    assert.deepEqual(await readdir(sharedRoot), []);

    const newRoot = join(parent, "private-root");
    const written = await writeValidRun(newRoot);
    assert.equal((await stat(newRoot)).mode & 0o777, 0o700);
    assert.equal((await stat(written.directory)).mode & 0o777, 0o700);
  } finally {
    await rm(sharedRoot, { recursive: true, force: true });
    await rm(parent, { recursive: true, force: true });
  }
});

test("writer rejects a symlink root", async () => {
  const parent = await mkdtemp(join(tmpdir(), "starring-slo-parent-"));
  try {
    const actualRoot = join(parent, "actual-root");
    const linkedRoot = join(parent, "linked-root");
    await mkdir(actualRoot, { mode: 0o700 });
    await symlink(actualRoot, linkedRoot, "dir");
    await assert.rejects(
      writeValidRun(linkedRoot),
      isArtifactError("invalid_artifact_root"),
    );
    assert.deepEqual(await readdir(actualRoot), []);
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});

test("artifact serialization rejects secret fields and permits token usage counts", () => {
  for (const key of [
    "authorization",
    "github_token",
    "DATABASE_SECRET",
    "OPENAI_API_KEY",
  ]) {
    assert.throws(
      () => assertSecretFree({ [key]: "hidden" }),
      isArtifactError("secret_field_forbidden"),
    );
  }
  assert.throws(
    () => assertSecretFree({ value: "Bearer hidden-token" }),
    isArtifactError("secret_material_forbidden"),
  );
  assert.deepEqual(assertSecretFree({
    input_tokens: 10,
    cached_input_tokens: 4,
    output_tokens: 2,
    reasoning_output_tokens: 1,
  }), {
    input_tokens: 10,
    cached_input_tokens: 4,
    output_tokens: 2,
    reasoning_output_tokens: 1,
  });
});
