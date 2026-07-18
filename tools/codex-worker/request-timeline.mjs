const NANOSECONDS_PER_MILLISECOND = 1_000_000n;
const TERMINAL_STAGES = new Set([
  "admission",
  "completed",
  "queue",
  "result_validation",
  "runner",
]);

function nonnegativeInteger(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`invalid_${name}`);
  }
  return value;
}

function monotonicClock() {
  return process.hrtime.bigint();
}

export class RequestTimeline {
  constructor(options = {}) {
    this.clock = options.clock ?? monotonicClock;
    if (typeof this.clock !== "function") {
      throw new TypeError("invalid_timeline_clock");
    }
    this.admittedAt = null;
    this.lastTimestamp = null;
    this.activeAtAdmission = null;
    this.queuedAtAdmission = null;
    this.submissionKind = null;
    this.runnerStartedAt = null;
    this.runnerSettledAt = null;
    this.runnerOutcome = null;
    this.validationStarted = false;
    this.result = null;
  }

  read() {
    const timestamp = this.clock();
    if (typeof timestamp !== "bigint" || timestamp < 0n) {
      throw new RangeError("invalid_monotonic_timestamp");
    }
    if (this.lastTimestamp !== null && timestamp < this.lastTimestamp) {
      throw new RangeError("non_monotonic_timestamp");
    }
    this.lastTimestamp = timestamp;
    return timestamp;
  }

  offset(timestamp) {
    const milliseconds = (timestamp - this.admittedAt) / NANOSECONDS_PER_MILLISECOND;
    const value = Number(milliseconds);
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new RangeError("invalid_timeline_duration");
    }
    return value;
  }

  admit(active, queued) {
    if (this.admittedAt !== null || this.result !== null) {
      throw new Error("invalid_admission_transition");
    }
    this.activeAtAdmission = nonnegativeInteger(active, "active_at_admission");
    this.queuedAtAdmission = nonnegativeInteger(queued, "queued_at_admission");
    this.admittedAt = this.read();
  }

  submissionObserved(active, queued) {
    if (this.admittedAt === null || this.submissionKind !== null || this.result !== null) {
      throw new Error("invalid_submission_transition");
    }
    const activeAfter = nonnegativeInteger(active, "active_after_submission");
    const queuedAfter = nonnegativeInteger(queued, "queued_after_submission");
    if (activeAfter > this.activeAtAdmission) {
      this.submissionKind = "active";
    } else if (queuedAfter > this.queuedAtAdmission) {
      this.submissionKind = "queued";
    } else {
      this.submissionKind = "rejected";
    }
  }

  runnerStarted() {
    if (this.result !== null) {
      return false;
    }
    if (this.admittedAt === null || this.runnerStartedAt !== null) {
      throw new Error("invalid_runner_start_transition");
    }
    this.runnerStartedAt = this.read();
    return true;
  }

  runnerSettled(outcome) {
    if (outcome !== "resolved" && outcome !== "rejected") {
      throw new RangeError("invalid_runner_outcome");
    }
    if (this.result !== null) {
      return false;
    }
    if (this.runnerStartedAt === null || this.runnerSettledAt !== null) {
      throw new Error("invalid_runner_settlement_transition");
    }
    this.runnerSettledAt = this.read();
    this.runnerOutcome = outcome;
    return true;
  }

  resultValidationStarted() {
    if (this.result !== null) {
      return false;
    }
    if (this.runnerOutcome !== "resolved" || this.validationStarted) {
      throw new Error("invalid_result_validation_transition");
    }
    this.validationStarted = true;
    return true;
  }

  failureStage() {
    if (this.runnerStartedAt !== null) {
      return this.validationStarted ? "result_validation" : "runner";
    }
    return this.submissionKind === "queued" ? "queue" : "admission";
  }

  finish(terminalStage) {
    if (!TERMINAL_STAGES.has(terminalStage)) {
      throw new RangeError("invalid_terminal_stage");
    }
    if (terminalStage === "completed"
      && (this.runnerOutcome !== "resolved" || !this.validationStarted)) {
      throw new Error("invalid_completed_transition");
    }
    if (terminalStage === "result_validation" && !this.validationStarted) {
      throw new Error("invalid_result_validation_terminal");
    }
    if (this.result !== null) {
      return this.result;
    }
    if (this.admittedAt === null || this.submissionKind === null) {
      throw new Error("incomplete_timeline");
    }
    const terminalAt = this.read();
    const totalDurationMs = this.offset(terminalAt);
    const runnerStarted = this.runnerStartedAt !== null;
    const runnerSettled = this.runnerSettledAt !== null;
    const runnerStartMs = runnerStarted ? this.offset(this.runnerStartedAt) : null;
    const runnerSettleMs = runnerSettled ? this.offset(this.runnerSettledAt) : null;
    const queueWaitMs = terminalStage === "admission"
      ? 0
      : runnerStartMs ?? totalDurationMs;
    const runnerDurationMs = runnerSettled
      ? runnerSettleMs - runnerStartMs
      : null;
    const postRunnerMs = runnerSettled
      ? totalDurationMs - runnerSettleMs
      : terminalStage === "admission"
        ? totalDurationMs
        : runnerStarted
          ? null
          : 0;
    const runnerElapsedAtTerminalMs = runnerStarted && !runnerSettled
      ? totalDurationMs - runnerStartMs
      : null;
    const decomposed = queueWaitMs
      + (runnerDurationMs ?? runnerElapsedAtTerminalMs ?? 0)
      + (postRunnerMs ?? 0);
    if (decomposed !== totalDurationMs) {
      throw new Error("invalid_timeline_decomposition");
    }
    this.result = Object.freeze({
      active_at_admission: this.activeAtAdmission,
      queued_at_admission: this.queuedAtAdmission,
      queue_wait_ms: queueWaitMs,
      runner_duration_ms: runnerDurationMs,
      runner_elapsed_at_terminal_ms: runnerElapsedAtTerminalMs,
      post_runner_ms: postRunnerMs,
      total_duration_ms: totalDurationMs,
      runner_started: runnerStarted,
      runner_settled: runnerSettled,
      runner_outcome: this.runnerOutcome,
      result_validation_started: this.validationStarted,
      terminal_stage: terminalStage,
    });
    return this.result;
  }
}
