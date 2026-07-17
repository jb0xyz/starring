import { execFile as execFileCallback } from "node:child_process";
import { homedir, machine, platform } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { assessRun } from "./acceptance.mjs";
import {
  releaseEvidenceRunReservation,
  reserveEvidenceRun,
  validateArtifactToolchain,
  verifyEvidenceRun,
  writeEvidenceRun,
} from "./artifact-store.mjs";
import { runPlan } from "./load-runner.mjs";
import { createFileMetricsReader } from "./metrics-reader.mjs";
import { getPlan } from "./plans.mjs";
import { createResourceSampler } from "./resource-sampler.mjs";
import { summarizeRun } from "./summarize.mjs";
import { workerSourceSha256 } from "../../tools/codex-worker/worker.mjs";

const execFile = promisify(execFileCallback);
const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export class SloProgramError extends Error {
  constructor(code) {
    super(code);
    this.name = "SloProgramError";
    this.code = code;
  }
}

function runId() {
  return `slo-${new Date().toISOString().toLowerCase().replace(/[^a-z0-9]+/g, "-")}`
    .replace(/-+$/, "");
}

export async function detectGitSource(options = {}) {
  const invoke = options.execFile ?? execFile;
  const digestWorker = options.workerDigest ?? workerSourceSha256;
  const repository = resolve(options.repository ?? REPOSITORY_ROOT);
  const snapshot = async () => {
    let commit;
    let status;
    try {
      commit = await invoke("git", ["rev-parse", "HEAD"], {
        cwd: repository,
        encoding: "utf8",
        maxBuffer: 1_000_000,
        timeout: 5_000,
      });
      status = await invoke("git", ["status", "--porcelain=v1", "--untracked-files=all"], {
        cwd: repository,
        encoding: "utf8",
        maxBuffer: 1_000_000,
        timeout: 5_000,
      });
    } catch {
      throw new SloProgramError("git_source_unavailable");
    }
    const sha = commit.stdout.trim();
    if (!/^[0-9a-f]{40}$/.test(sha)) {
      throw new SloProgramError("git_source_invalid");
    }
    if (status.stdout.length !== 0) {
      throw new SloProgramError("dirty_source_forbidden");
    }
    return sha;
  };
  const before = await snapshot();
  let digest;
  try {
    digest = await digestWorker();
  } catch {
    throw new SloProgramError("worker_source_unavailable");
  }
  if (!/^[0-9a-f]{64}$/.test(digest)) {
    throw new SloProgramError("worker_source_invalid");
  }
  const after = await snapshot();
  if (before !== after) {
    throw new SloProgramError("git_source_changed");
  }
  return { commit: before, dirty: false, worker_source_sha256: digest };
}

async function prerequisite(plan, source, directory) {
  if (plan.id !== "step_load_diagnostic") {
    return null;
  }
  if (typeof directory !== "string" || directory.length === 0) {
    throw new SloProgramError("canary_prerequisite_required");
  }
  const evidence = await verifyEvidenceRun(directory);
  if (evidence.acceptance.plan_id !== "live_canary"
    || evidence.acceptance.verdict !== "pass"
    || evidence.acceptance.claims?.eligible_for_step_load !== true
    || evidence.manifest.source_commit !== source.commit
    || evidence.manifest.worker_source_sha256 !== source.worker_source_sha256
    || evidence.raw.source.worker_source_sha256 !== source.worker_source_sha256) {
    throw new SloProgramError("canary_prerequisite_invalid");
  }
  return evidence.manifest.run_id;
}

export async function executeSloProgram(options) {
  const plan = getPlan(options.planName);
  if (plan.id === "commercial_candidate") {
    throw new SloProgramError("commercial_connectors_not_bound");
  }
  if (plan.execution_mode === "live" && options.source !== undefined) {
    throw new SloProgramError("live_source_override_forbidden");
  }
  const source = options.source ?? await detectGitSource({
    repository: options.repository,
    execFile: options.execFile,
    workerDigest: options.workerDigest,
  });
  const prerequisiteRunId = await prerequisite(
    plan,
    source,
    options.prerequisiteDirectory,
  );
  const selectedRunId = options.runId ?? runId();
  const outputRoot = options.outputRoot
    ?? join(homedir(), "Library", "Application Support", "Starring", "slo-evidence");
  const toolchain = validateArtifactToolchain(options.toolchain ?? {
    node: process.version,
    platform: platform(),
    machine: machine(),
    evaluator_revision: 1,
  });
  let metricsReader = options.metricsReader;
  if (plan.execution_mode === "live" && metricsReader === undefined) {
    metricsReader = await createFileMetricsReader({
      path: options.metricsPath
        ?? join(homedir(), "Library", "Logs", "Starring", "codex-worker.jsonl"),
      backups: options.metricsBackups,
    });
  }
  let resourceSampler = options.resourceSampler;
  if (resourceSampler === undefined && Number.isSafeInteger(options.workerPid)) {
    resourceSampler = createResourceSampler({
      pid: options.workerPid,
      intervalMs: plan.resource_sampling.interval_ms,
      maximumSamples: plan.resource_sampling.maximum_samples,
    });
  }
  const reservation = await reserveEvidenceRun({
    rootDirectory: outputRoot,
    runId: selectedRunId,
  });
  let raw;
  try {
    raw = await runPlan({
      plan,
      source,
      runId: selectedRunId,
      baseUrl: options.baseUrl,
      token: options.token,
      fetchFn: options.fetchFn,
      productExecutor: options.productExecutor,
      metricsReader,
      scenarioResults: options.scenarioResults,
      resourceSampler,
      healthPollMs: options.healthPollMs,
      cancellationAdmissionTimeoutMs: options.cancellationAdmissionTimeoutMs,
      metricsHealthTimeoutMs: options.metricsHealthTimeoutMs,
      resourceCleanupTimeoutMs: options.resourceCleanupTimeoutMs,
      wallClock: options.wallClock,
      clock: options.clock,
    });
  } catch (error) {
    await releaseEvidenceRunReservation(reservation);
    throw error;
  }
  raw.prerequisite_run_id = prerequisiteRunId;
  if (plan.execution_mode === "live") {
    try {
      const sourceEnd = await detectGitSource({
        repository: options.repository,
        execFile: options.execFile,
        workerDigest: options.workerDigest,
      });
      raw.source_end = sourceEnd;
      if (sourceEnd.commit !== source.commit
        || sourceEnd.worker_source_sha256 !== source.worker_source_sha256) {
        raw.interrupted = true;
        raw.stop_reason ??= "source_continuity_lost";
      }
    } catch (error) {
      raw.source_end = { error_code: error?.code ?? "source_revalidation_failed" };
      raw.interrupted = true;
      raw.stop_reason ??= "source_continuity_lost";
    }
  }
  const summary = summarizeRun(raw);
  const acceptance = assessRun(plan, raw, summary);
  const written = await writeEvidenceRun({
    reservation,
    raw,
    summary,
    acceptance,
    toolchain,
  });
  const verified = await verifyEvidenceRun(written.directory);
  return {
    directory: written.directory,
    raw: verified.raw,
    summary: verified.summary,
    acceptance: verified.acceptance,
    manifest: verified.manifest,
  };
}

function integerEnvironment(value, name) {
  if (value === undefined || value === "") {
    return undefined;
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new SloProgramError(`invalid_${name}`);
  }
  return parsed;
}

async function runMain() {
  const planName = process.argv[2];
  const result = await executeSloProgram({
    planName,
    baseUrl: process.env.STARRING_CODEX_WORKER_URL ?? "http://127.0.0.1:18181",
    token: process.env.STARRING_CODEX_WORKER_TOKEN,
    metricsPath: process.env.STARRING_CODEX_WORKER_METRICS_LOG,
    outputRoot: process.env.STARRING_SLO_OUTPUT_ROOT,
    prerequisiteDirectory: process.env.STARRING_SLO_PREREQUISITE,
    workerPid: integerEnvironment(process.env.STARRING_CODEX_WORKER_PID, "worker_pid"),
  });
  process.stdout.write(`${JSON.stringify({
    run_id: result.raw.run_id,
    plan_id: result.raw.plan.id,
    verdict: result.acceptance.verdict,
    stop_reason: result.raw.stop_reason,
    directory: result.directory,
    claims: result.acceptance.claims,
    usage: result.summary.usage,
    latency: result.summary.latency,
  })}\n`);
  if (result.acceptance.verdict !== "pass") {
    process.exitCode = 2;
  }
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : null;
if (invokedPath === import.meta.url) {
  runMain().catch((error) => {
    const code = typeof error?.code === "string" ? error.code : "slo_program_failed";
    process.stderr.write(`${JSON.stringify({ event: "slo_program_failed", code })}\n`);
    process.exitCode = 1;
  });
}
