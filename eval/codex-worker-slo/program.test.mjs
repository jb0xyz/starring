import assert from "node:assert/strict";
import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { SloProgramError, detectGitSource, executeSloProgram } from "./program.mjs";

test("git source detection requires one clean exact commit", async () => {
  const calls = [];
  const clean = await detectGitSource({
    repository: "/tmp/repository",
    execFile: async (_command, arguments_) => {
      calls.push(arguments_[0]);
      return {
        stdout: arguments_[0] === "rev-parse" ? `${"a".repeat(40)}\n` : "",
        stderr: "",
      };
    },
    workerDigest: async () => "b".repeat(64),
  });
  assert.deepEqual(clean, {
    commit: "a".repeat(40),
    dirty: false,
    worker_source_sha256: "b".repeat(64),
  });
  assert.deepEqual(calls, ["rev-parse", "status", "rev-parse", "status"]);
  await assert.rejects(
    detectGitSource({
      repository: "/tmp/repository",
      execFile: async (_command, arguments_) => ({
        stdout: arguments_[0] === "rev-parse" ? `${"a".repeat(40)}\n` : " M changed\n",
        stderr: "",
      }),
      workerDigest: async () => "b".repeat(64),
    }),
    (error) => error instanceof SloProgramError && error.code === "dirty_source_forbidden",
  );
  let revision = 0;
  await assert.rejects(
    detectGitSource({
      repository: "/tmp/repository",
      execFile: async (_command, arguments_) => ({
        stdout: arguments_[0] === "rev-parse"
          ? `${(revision++ === 0 ? "a" : "c").repeat(40)}\n`
          : "",
      stderr: "",
      }),
      workerDigest: async () => "b".repeat(64),
    }),
    (error) => error instanceof SloProgramError && error.code === "git_source_changed",
  );
});

test("program seals and verifies one zero-token development artifact", async () => {
  const privateParent = await mkdtemp(join(tmpdir(), "starring-slo-program-"));
  const outputRoot = join(privateParent, "evidence");
  try {
    const result = await executeSloProgram({
      planName: "development",
      source: { commit: "a".repeat(40), dirty: false },
      runId: "development-program-test",
      outputRoot,
      healthPollMs: 1,
      wallClock: () => "2026-07-17T00:00:00.000Z",
      toolchain: { node: process.version, evaluator_revision: 1 },
    });
    assert.equal(result.raw.observed_live_calls, 0);
    assert.equal(result.raw.automatic_retries, 0);
    assert.equal(result.manifest.run_id, "development-program-test");
    assert.equal(result.acceptance.claims.commercial_slo_certified, false);
  } finally {
    await rm(privateParent, { recursive: true, force: true });
  }
});

test("program reserves evidence and validates toolchain before execution", async () => {
  const privateParent = await mkdtemp(join(tmpdir(), "starring-slo-preflight-"));
  try {
    await assert.rejects(
      executeSloProgram({
        planName: "development",
        source: { commit: "a".repeat(40), dirty: false },
        runId: "missing-parent",
        outputRoot: join(privateParent, "missing", "evidence"),
      }),
      (error) => error.code === "artifact_root_parent_missing",
    );
    await assert.rejects(
      executeSloProgram({
        planName: "development",
        source: { commit: "a".repeat(40), dirty: false },
        runId: "invalid-toolchain",
        outputRoot: join(privateParent, "evidence"),
        toolchain: { node: process.version, evaluator_revision: 1, credential: "hidden" },
      }),
      (error) => error.code === "invalid_toolchain",
    );
    assert.deepEqual(await readdir(privateParent), []);
  } finally {
    await rm(privateParent, { recursive: true, force: true });
  }
});
