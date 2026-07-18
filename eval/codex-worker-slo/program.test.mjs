import assert from "node:assert/strict";
import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { SloProgramError, detectGitSource, executeSloProgram } from "./program.mjs";
import { summarizeRun } from "./summarize.mjs";

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

test("live programs reject all executor overrides before source or transport access", async () => {
  for (const override of [
    { runPlanExecutor: async () => ({}) },
    { summarizeExecutor: () => ({}) },
    { assessExecutor: () => ({}) },
  ]) {
    await assert.rejects(
      executeSloProgram({
        planName: "live_canary",
        ...override,
      }),
      (error) => error instanceof SloProgramError
        && error.code === "live_executor_override_forbidden",
    );
  }
});

test("program seals fail-closed evidence when execution throws", async () => {
  const privateParent = await mkdtemp(join(tmpdir(), "starring-slo-execution-failure-"));
  try {
    const result = await executeSloProgram({
      planName: "development",
      source: { commit: "a".repeat(40), dirty: false },
      runId: "execution-failure-evidence",
      outputRoot: join(privateParent, "evidence"),
      toolchain: { node: process.version, evaluator_revision: 1 },
      runPlanExecutor: async () => {
        const error = new Error("bounded");
        error.code = "post_provider_failure";
        throw error;
      },
    });
    assert.equal(result.acceptance.verdict, "fail");
    assert.equal(result.raw.stop_reason, "slo_execution_failed");
    assert.equal(result.raw.execution_error_code, "post_provider_failure");
    assert.equal(result.raw.evidence_completeness, "execution_exception_fallback");
    assert.ok(result.acceptance.non_claims.includes("execution_evidence_incomplete"));
    assert.ok(result.acceptance.non_claims.includes(
      "live_call_and_usage_observation_incomplete",
    ));
  } finally {
    await rm(privateParent, { recursive: true, force: true });
  }
});

test("program retries one fail-closed seal after a postprocess exception", async () => {
  const privateParent = await mkdtemp(join(tmpdir(), "starring-slo-postprocess-failure-"));
  let summaries = 0;
  try {
    const result = await executeSloProgram({
      planName: "development",
      source: { commit: "a".repeat(40), dirty: false },
      runId: "postprocess-failure-evidence",
      outputRoot: join(privateParent, "evidence"),
      toolchain: { node: process.version, evaluator_revision: 1 },
      summarizeExecutor: (raw) => {
        summaries += 1;
        if (summaries === 1) {
          const error = new Error("bounded");
          error.code = "summary_failed";
          throw error;
        }
        return summarizeRun(raw);
      },
    });
    assert.equal(summaries, 2);
    assert.equal(result.acceptance.verdict, "fail");
    assert.equal(result.raw.stop_reason, "slo_postprocess_failed");
    assert.equal(result.raw.postprocess_error_code, "summary_failed");
    assert.equal(result.raw.evidence_completeness, "postprocess_fallback");
  } finally {
    await rm(privateParent, { recursive: true, force: true });
  }
});

test("unsealable failures retain their run id and reserved directory", async () => {
  const privateParent = await mkdtemp(join(tmpdir(), "starring-slo-sealing-failure-"));
  try {
    await assert.rejects(
      executeSloProgram({
        planName: "development",
        source: { commit: "a".repeat(40), dirty: false },
        runId: "unsealable-failure",
        outputRoot: join(privateParent, "evidence"),
        toolchain: { node: process.version, evaluator_revision: 1 },
        summarizeExecutor: () => {
          const error = new Error("bounded");
          error.code = "summary_failed";
          throw error;
        },
      }),
      (error) => error instanceof SloProgramError
        && error.code === "evidence_sealing_failed"
        && error.run_id === "unsealable-failure"
        && error.directory === join(privateParent, "evidence", "unsealable-failure"),
    );
  } finally {
    await rm(privateParent, { recursive: true, force: true });
  }
});
