import { createHash } from "node:crypto";
import { WorkerError } from "./protocol.mjs";

const OBSERVATION_HEADER = "x-starring-observation-id";

const OBSERVATION_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function validateObservationId(value) {
  if (typeof value !== "string" || !OBSERVATION_ID_PATTERN.test(value)) {
    throw new WorkerError("invalid_observation_id", 400);
  }
  return value;
}

export function requestObservationId(request) {
  const value = request.headers[OBSERVATION_HEADER];
  return value === undefined ? null : validateObservationId(value);
}

function digestObservationId(value) {
  return createHash("sha256").update(value).digest("hex");
}

export class AdmissionRegistry {
  constructor(options = {}) {
    this.clock = options.clock ?? Date.now;
    this.ttlMs = options.ttlMs ?? 60_000;
    this.capacity = options.capacity ?? 1_024;
    if (typeof this.clock !== "function"
      || !Number.isSafeInteger(this.ttlMs)
      || this.ttlMs < 1
      || !Number.isSafeInteger(this.capacity)
      || this.capacity < 1) {
      throw new TypeError("invalid_admission_registry_options");
    }
    this.live = new Map();
    this.tombstones = new Map();
  }

  prune(now = this.clock()) {
    for (const [digest, expiresAt] of this.tombstones) {
      if (expiresAt > now) {
        continue;
      }
      this.tombstones.delete(digest);
    }
  }

  reserve(observationId, requestId) {
    this.prune();
    const digest = digestObservationId(observationId);
    if (this.live.has(observationId) || this.tombstones.has(digest)) {
      throw new WorkerError("observation_id_collision", 409);
    }
    if (this.live.size + this.tombstones.size >= this.capacity) {
      throw new WorkerError("observation_registry_full", 503);
    }
    this.live.set(observationId, {
      digest,
      requestId,
      status: null,
    });
  }

  admit(observationId, status) {
    if (observationId === null) {
      return;
    }
    if (status !== "active" && status !== "queued") {
      throw new TypeError("invalid_admission_status");
    }
    const entry = this.live.get(observationId);
    if (!entry || entry.status !== null) {
      throw new Error("invalid_admission_transition");
    }
    entry.status = status;
  }

  activate(observationId) {
    if (observationId === null) {
      return;
    }
    const entry = this.live.get(observationId);
    if (!entry || (entry.status !== "active" && entry.status !== "queued")) {
      throw new Error("invalid_activation_transition");
    }
    entry.status = "active";
  }

  lookup(observationId) {
    this.prune();
    const entry = this.live.get(observationId);
    if (!entry || (entry.status !== "active" && entry.status !== "queued")) {
      return null;
    }
    return {
      schema_version: 1,
      observation_id: observationId,
      status: entry.status,
      request_id: entry.requestId,
    };
  }

  release(observationId) {
    if (observationId === null) {
      return;
    }
    const entry = this.live.get(observationId);
    if (!entry) {
      return;
    }
    this.live.delete(observationId);
    this.prune();
    this.tombstones.set(entry.digest, this.clock() + this.ttlMs);
  }
}
