import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";


const SOURCE = await readFile(new URL("./product_driver.js", import.meta.url), "utf8");
const DIGEST = "a".repeat(64);
const PROCESS_INSTANCE_ID = "0123456789abcdef0123456789abcdef";


function response(status, body) {
  return {
    status,
    ok: status >= 200 && status < 300,
    text: async () => (body === null ? "" : JSON.stringify(body)),
  };
}


function driver(fetchImpl, cookie = "__Host-starring_csrf=csrf-value", options = {}) {
  const context = {
    URL,
    Error,
    Object,
    Promise,
    AbortController,
    console,
    globalThis: null,
    location: { origin: "https://d2-api.starring.co.kr" },
    setTimeout,
    clearTimeout,
  };
  context.globalThis = context;
  vm.runInNewContext(SOURCE, context, { filename: "product_driver.js" });
  return context.StarringD2ProductDriver.create({
    fetchImpl,
    cookieSource: () => cookie,
    randomUUID: () => "00000000-0000-4000-8000-000000000001",
    sleep: async () => {},
    ...options,
  });
}


test("Discord interaction observation is canonical and exact", () => {
  const evidence = driver(async () => response(500, null), undefined, {
    now: () => "2026-08-04T12:00:00Z",
  }).discordInteractionObservation({
    guildId: "1524810437118525551",
    resourcePrefix: "starring-d2-abcdef123456",
    actorUserId: "1056857223529250906",
    createInteractionId: "1524810437118525560",
    joinInteractionId: "1524810437118525561",
    joinedRoleId: "1524810437118525570",
    roleIds: ["1524810437118525571", "1524810437118525570"],
    channelIds: ["1524810437118525580"],
    panelMessageIds: ["1524810437118525590"],
    createResponseObserved: true,
    joinResponseObserved: true,
    privateChannelObserved: true,
    roleAssignmentObserved: true,
    joinPanelObserved: true,
  });
  assert.deepEqual(
    { ...evidence },
    {
      schema_version: 1,
      kind: "starring.d2.browser-discord-interaction-observation.v1",
      observed_at: "2026-08-04T12:00:00Z",
      guild_id: "1524810437118525551",
      resource_prefix: "starring-d2-abcdef123456",
      actor_user_id: "1056857223529250906",
      create_interaction_id: "1524810437118525560",
      join_interaction_id: "1524810437118525561",
      joined_role_id: "1524810437118525570",
      role_ids: ["1524810437118525570", "1524810437118525571"],
      channel_ids: ["1524810437118525580"],
      panel_message_ids: ["1524810437118525590"],
      create_response_observed: true,
      join_response_observed: true,
      private_channel_observed: true,
      role_assignment_observed: true,
      join_panel_observed: true,
      confirmation_surface: "chrome_discord_web",
    },
  );
});


test("Discord interaction observation rejects weak or ambiguous confirmation", () => {
  const product = driver(async () => response(500, null), undefined, {
    now: () => "2026-08-04T12:00:00Z",
  });
  const valid = {
    guildId: "1524810437118525551",
    resourcePrefix: "starring-d2-abcdef123456",
    actorUserId: "1056857223529250906",
    createInteractionId: "1524810437118525560",
    joinInteractionId: "1524810437118525561",
    joinedRoleId: "1524810437118525570",
    roleIds: ["1524810437118525570"],
    channelIds: ["1524810437118525580"],
    panelMessageIds: ["1524810437118525590"],
    createResponseObserved: true,
    joinResponseObserved: true,
    privateChannelObserved: true,
    roleAssignmentObserved: true,
    joinPanelObserved: true,
  };
  assert.throws(
    () => product.discordInteractionObservation({
      ...valid,
      joinInteractionId: valid.createInteractionId,
    }),
    /discord_interaction_identity_invalid/,
  );
  assert.throws(
    () => product.discordInteractionObservation({
      ...valid,
      roleIds: [valid.channelIds[0]],
    }),
    /discord_interaction_observation_invalid/,
  );
  assert.throws(
    () => product.discordInteractionObservation({
      ...valid,
      joinPanelObserved: false,
    }),
    /discord_interaction_observation_invalid/,
  );
  assert.throws(
    () => product.discordInteractionObservation({
      ...valid,
      joinedRoleId: "1524810437118525572",
    }),
    /discord_joined_role_invalid/,
  );
});


test("one-shot flow uses product boundaries and returns no prompt or full preview ruleset", async () => {
  const calls = [];
  const responses = [
    response(201, {
      session_id: "session-1",
      generation: 1,
      disposition: "created",
      projection: {
        state: "preview_ready",
        assistant_message: "not retained",
        preview: {
          revision: 1,
          ruleset: { rules: [{ hidden: true }] },
          receipt: { candidate_ruleset_hash: DIGEST },
        },
      },
    }),
    response(201, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      summary: {
        panels: 1,
        modals: 1,
        rules: 4,
        actions: 15,
        target_version: 1,
        required_approvals: 1,
        target_content_hash: DIGEST,
        binding_fingerprint: DIGEST,
        expires_at: "2026-08-01T12:00:00Z",
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "approved",
      replayed: false,
    }),
    response(202, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "runtime_pending",
      replayed: false,
    }),
  ];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options });
    return responses.shift();
  };
  const evidence = await driver(fetchImpl).runOneShotProductFlow({
    installationId: "installation-1",
    sessionId: "session-1",
    message: "Create the private study room automation",
    confirmPreview: async (preview) => {
      assert.equal(preview.promotion_id, DIGEST);
      assert.deepEqual(
        { ...preview.summary },
        {
          panels: 1,
          modals: 1,
          rules: 4,
          actions: 15,
          target_version: 1,
          required_approvals: 1,
        }
      );
      return true;
    },
  });
  assert.equal(calls.length, 5);
  assert.equal(calls[0].options.headers["x-csrf-token"], "csrf-value");
  assert.equal(calls[0].options.credentials, "same-origin");
  assert.equal(calls[4].options.method, "POST");
  assert.equal(evidence.authoring.projection_state, "preview_ready");
  assert.equal(evidence.applied.state, "runtime_pending");
  assert.deepEqual(
    Object.keys(evidence.authoring_evidence).sort(),
    [
      "authoring_generation",
      "authoring_http_status",
      "authoring_session_id",
      "installation_id",
      "kind",
      "observed_at",
      "one_shot",
      "public_origin",
      "schema_version",
    ]
  );
  assert.equal(evidence.authoring_evidence.kind, "starring.d2.browser-authoring-evidence.v1");
  assert.equal(evidence.authoring_evidence.authoring_http_status, 201);
  assert.equal(evidence.authoring_evidence.authoring_session_id, "session-1");
  assert.equal(evidence.authoring_evidence.authoring_generation, 1);
  assert.equal(evidence.authoring_evidence.installation_id, "installation-1");
  assert.equal(evidence.authoring_evidence.one_shot, true);
  assert.deepEqual(
    Object.keys(evidence.product_decision_evidence).sort(),
    [
      "apply_state",
      "approval_state",
      "authoring_generation",
      "authoring_session_id",
      "installation_id",
      "kind",
      "observed_at",
      "payload_digest",
      "preview_state",
      "promotion_id",
      "public_origin",
      "runtime_pending_observed",
      "schema_version",
    ]
  );
  assert.equal(
    evidence.product_decision_evidence.kind,
    "starring.d2.browser-product-decision-evidence.v1"
  );
  assert.equal(evidence.product_decision_evidence.preview_state, "pending_approval");
  assert.equal(evidence.product_decision_evidence.authoring_session_id, "session-1");
  assert.equal(evidence.product_decision_evidence.authoring_generation, 1);
  assert.equal(evidence.product_decision_evidence.payload_digest, DIGEST);
  assert.equal(evidence.product_decision_evidence.approval_state, "approved");
  assert.equal(evidence.product_decision_evidence.apply_state, "runtime_pending");
  assert.equal(evidence.product_decision_evidence.runtime_pending_observed, true);
  const serialized = JSON.stringify(evidence);
  assert.equal(serialized.includes("Create the private study room automation"), false);
  assert.equal(serialized.includes("assistant_message"), false);
  assert.equal(serialized.includes("csrf-value"), false);
  assert.equal(serialized.includes("request_id"), false);
  assert.equal(serialized.includes("provider"), false);
  assert.equal(serialized.includes("reasoning_effort"), false);
  assert.equal(serialized.includes("auth_mode"), false);
  assert.equal(serialized.includes("ruleset"), true);
  assert.equal(serialized.includes('"hidden"'), false);
  assert.equal(Object.hasOwn(evidence.preview, "ruleset"), false);
  assert.equal(Object.hasOwn(evidence.preview.summary, "target_content_hash"), false);
  assert.equal(Object.hasOwn(evidence.preview.summary, "binding_fingerprint"), false);
  assert.equal(Object.hasOwn(evidence.preview.summary, "expires_at"), false);
});


test("one-shot exact replay emits strict redacted evidence when apply is already live", async () => {
  const responses = [
    response(200, {
      session_id: "session-replay",
      generation: 4,
      disposition: "exact_replay",
      projection: {
        state: "preview_ready",
        assistant_message: "secret assistant output",
        preview: {
          revision: 4,
          ruleset: { secret: "not retained" },
          receipt: { candidate_ruleset_hash: DIGEST },
        },
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      replayed: true,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      summary: {
        panels: 1,
        modals: 1,
        rules: 4,
        actions: 15,
        target_version: 1,
        required_approvals: 1,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "approved",
      replayed: true,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      replayed: true,
    }),
  ];
  const evidence = await driver(async () => responses.shift(), undefined, {
    now: () => "2026-08-04T12:00:00Z",
  }).runOneShotProductFlow({
    installationId: "installation-1",
    sessionId: "session-replay",
    expectedGeneration: 3,
    message: "secret replay prompt",
    confirmPreview: async () => true,
  });
  assert.equal(evidence.authoring_evidence.authoring_http_status, 200);
  assert.equal(evidence.authoring_evidence.authoring_generation, 4);
  assert.equal(evidence.authoring_evidence.one_shot, true);
  assert.equal(evidence.product_decision_evidence.apply_state, "live");
  assert.equal(evidence.product_decision_evidence.authoring_session_id, "session-replay");
  assert.equal(evidence.product_decision_evidence.authoring_generation, 4);
  assert.equal(evidence.product_decision_evidence.payload_digest, DIGEST);
  assert.equal(evidence.product_decision_evidence.runtime_pending_observed, false);
  assert.equal(evidence.runtime_pending_observed, false);
  const serialized = JSON.stringify({
    authoring: evidence.authoring_evidence,
    decision: evidence.product_decision_evidence,
  });
  assert.equal(serialized.includes("secret replay prompt"), false);
  assert.equal(serialized.includes("secret assistant output"), false);
  assert.equal(serialized.includes("not retained"), false);
  assert.equal(serialized.includes("csrf-value"), false);
});


test("one-shot flow retries only the exact apply command during a runtime drain", async () => {
  const calls = [];
  const sleeps = [];
  let confirmations = 0;
  let uuidCounter = 0;
  const responses = [
    response(201, {
      session_id: "session-1",
      generation: 1,
      disposition: "created",
      projection: {
        state: "preview_ready",
        preview: { revision: 1, receipt: { candidate_ruleset_hash: DIGEST } },
      },
    }),
    response(201, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      summary: {
        panels: 1,
        modals: 1,
        rules: 4,
        actions: 15,
        target_version: 2,
        required_approvals: 1,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "approved",
      replayed: false,
    }),
    response(409, {
      error: {
        code: "runtime_drain_required",
        request_id: "request-1",
        retryable: true,
      },
    }),
    response(409, {
      error: {
        code: "runtime_drain_pending",
        request_id: "request-2",
        retryable: true,
      },
    }),
    response(202, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "runtime_pending",
      replayed: false,
    }),
  ];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options, body: options.body && JSON.parse(options.body) });
    return responses.shift();
  };
  const evidence = await driver(fetchImpl, undefined, {
    sleep: async (milliseconds) => sleeps.push(milliseconds),
    randomUUID: () => {
      uuidCounter += 1;
      return `00000000-0000-4000-8000-${String(uuidCounter).padStart(12, "0")}`;
    },
  }).runOneShotProductFlow({
    installationId: "installation-1",
    sessionId: "session-1",
    message: "Update the study room automation",
    confirmPreview: async () => {
      confirmations += 1;
      return true;
    },
    runtimeDrainAttempts: 3,
    runtimeDrainIntervalMilliseconds: 100,
  });
  assert.equal(calls.length, 7);
  assert.equal(confirmations, 1);
  assert.deepEqual(sleeps, [100, 100]);
  const applyCalls = calls.slice(4);
  assert.equal(applyCalls.length, 3);
  assert.equal(new Set(applyCalls.map((call) => call.url)).size, 1);
  assert.equal(new Set(applyCalls.map((call) => call.options.headers["idempotency-key"])).size, 1);
  assert.match(applyCalls[0].options.headers["idempotency-key"], /^d2\.apply\./);
  assert.deepEqual(applyCalls.map((call) => call.body), [
    { expected_payload_digest: DIGEST, expected_revision: 2 },
    { expected_payload_digest: DIGEST, expected_revision: 2 },
    { expected_payload_digest: DIGEST, expected_revision: 2 },
  ]);
  assert.equal(evidence.apply_attempts, 3);
  assert.equal(evidence.runtime_drain_observed, true);
  assert.equal(evidence.applied.state, "runtime_pending");
});


test("runtime drain handshake is bounded and does not retry unrelated conflicts", async () => {
  const drainCalls = [];
  const drainSleeps = [];
  const drainResponses = [
    response(409, {
      error: {
        code: "runtime_drain_required",
        request_id: "request-1",
        retryable: true,
      },
    }),
    response(409, {
      error: {
        code: "runtime_drain_pending",
        request_id: "request-2",
        retryable: true,
      },
    }),
  ];
  const draining = driver(async (url, options) => {
    drainCalls.push({ url, options });
    return drainResponses.shift();
  }, undefined, {
    sleep: async (milliseconds) => drainSleeps.push(milliseconds),
  });
  await assert.rejects(
    draining.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      idempotencyKey: "apply-stable-2",
      runtimeDrainAttempts: 2,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    (error) => error.code === "runtime_drain_pending"
  );
  assert.equal(drainCalls.length, 2);
  assert.deepEqual(drainSleeps, [100]);
  assert.equal(drainCalls[0].options.body, drainCalls[1].options.body);
  assert.equal(
    drainCalls[0].options.headers["idempotency-key"],
    drainCalls[1].options.headers["idempotency-key"]
  );

  let genericCalls = 0;
  let genericSleeps = 0;
  const generic = driver(async () => {
    genericCalls += 1;
    return response(409, {
      error: {
        code: "revision_conflict",
        request_id: "request-3",
        retryable: false,
      },
    });
  }, undefined, {
    sleep: async () => {
      genericSleeps += 1;
    },
  });
  await assert.rejects(
    generic.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    (error) => error.code === "revision_conflict"
  );
  assert.equal(genericCalls, 1);
  assert.equal(genericSleeps, 0);
});


test("runtime drain handshake defaults to bounded capped backoff with stable authority", async () => {
  const calls = [];
  const sleeps = [];
  let responseIndex = 0;
  const product = driver(async (url, options) => {
    calls.push({ url, options });
    responseIndex += 1;
    if (responseIndex <= 4) {
      return response(409, {
        error: {
          code: responseIndex === 1 ? "runtime_drain_required" : "runtime_drain_pending",
          request_id: `request-default-${responseIndex}`,
          retryable: true,
        },
      });
    }
    return response(202, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "runtime_pending",
      replayed: false,
    });
  }, undefined, {
    sleep: async (milliseconds) => sleeps.push(milliseconds),
  });

  const applied = await product.applyWithDrainHandshake({
    installationId: "installation-1",
    promotionId: DIGEST,
    expectedPayloadDigest: DIGEST,
    expectedRevision: 2,
    idempotencyKey: "apply-default-backoff",
  });

  assert.equal(applied.attempts, 5);
  assert.deepEqual(sleeps, [2000, 4000, 8000, 15000]);
  assert.equal(new Set(calls.map((call) => call.url)).size, 1);
  assert.equal(new Set(calls.map((call) => call.options.body)).size, 1);
  assert.equal(
    new Set(calls.map((call) => call.options.headers["idempotency-key"])).size,
    1
  );
});


test("runtime drain handshake default exhaustion emits eleven requests over 119 seconds", async () => {
  const calls = [];
  const sleeps = [];
  const product = driver(async (url, options) => {
    calls.push({ url, options });
    return response(409, {
      error: {
        code: "runtime_drain_pending",
        request_id: `request-exhausted-${calls.length}`,
        retryable: true,
      },
    });
  }, undefined, {
    sleep: async (milliseconds) => sleeps.push(milliseconds),
  });

  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      idempotencyKey: "apply-default-exhausted",
    }),
    (error) => error.code === "runtime_drain_pending"
  );

  assert.equal(calls.length, 11);
  assert.deepEqual(sleeps, [2000, 4000, 8000, 15000, 15000, 15000, 15000, 15000, 15000, 15000]);
  assert.equal(sleeps.reduce((total, value) => total + value, 0), 119000);
  assert.equal(new Set(calls.map((call) => call.options.body)).size, 1);
  assert.equal(
    new Set(calls.map((call) => call.options.headers["idempotency-key"])).size,
    1
  );
});


test("runtime drain handshake never retries a generic dependency failure", async () => {
  let calls = 0;
  let sleeps = 0;
  const product = driver(async () => {
    calls += 1;
    return response(503, {
      error: {
        code: "dependency_unavailable",
        request_id: "request-1",
        retryable: true,
      },
    });
  }, undefined, {
    sleep: async () => {
      sleeps += 1;
    },
  });
  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    (error) => error.status === 503 && error.code === "dependency_unavailable"
  );
  assert.equal(calls, 1);
  assert.equal(sleeps, 0);
});


test("invalid apply state resumes from the exact runtime pending promotion", async () => {
  const calls = [];
  const responses = [
    response(409, {
      error: {
        code: "invalid_state",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 4,
      state: "runtime_pending",
      payload_digest: DIGEST,
      apply_source_revision: 2,
      replayed: false,
    }),
  ];
  const product = driver(async (url, options) => {
    calls.push({ url, options });
    return responses.shift();
  });
  const applied = await product.applyWithDrainHandshake({
    installationId: "installation-1",
    promotionId: DIGEST,
    expectedPayloadDigest: DIGEST,
    expectedRevision: 2,
    runtimeDrainAttempts: 3,
    runtimeDrainIntervalMilliseconds: 100,
  });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].options.method, "POST");
  assert.equal(calls[1].options.method, "GET");
  assert.equal(applied.status, 200);
  assert.equal(applied.body.state, "runtime_pending");
  assert.equal(applied.attempts, 1);
  assert.equal(applied.runtime_pending_observed, true);
  assert.equal(applied.resumed_after_conflict, true);
  assert.equal(applied.status_observations, 1);
});


test("invalid apply state observes applying until the exact promotion is live", async () => {
  const calls = [];
  const sleeps = [];
  const responses = [
    response(409, {
      error: {
        code: "invalid_state",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 3,
      state: "applying",
      payload_digest: DIGEST,
      apply_source_revision: 2,
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 3,
      state: "applying",
      payload_digest: DIGEST,
      apply_source_revision: 2,
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 4,
      state: "live",
      payload_digest: DIGEST,
      apply_source_revision: 2,
      replayed: false,
    }),
  ];
  const product = driver(async (url, options) => {
    calls.push({ url, options });
    return responses.shift();
  }, undefined, {
    sleep: async (milliseconds) => sleeps.push(milliseconds),
  });
  const applied = await product.applyWithDrainHandshake({
    installationId: "installation-1",
    promotionId: DIGEST,
    expectedPayloadDigest: DIGEST,
    expectedRevision: 2,
    runtimeDrainAttempts: 3,
    runtimeDrainIntervalMilliseconds: 100,
  });
  assert.equal(calls.filter((call) => call.options.method === "POST").length, 1);
  assert.equal(calls.filter((call) => call.options.method === "GET").length, 3);
  assert.deepEqual(sleeps, [100, 100]);
  assert.equal(applied.body.state, "live");
  assert.equal(applied.attempts, 1);
  assert.equal(applied.runtime_pending_observed, false);
  assert.equal(applied.resumed_after_conflict, true);
  assert.equal(applied.status_observations, 3);
});


test("invalid apply state remains terminal when the exact promotion is not applying or applied", async () => {
  const calls = [];
  const responses = [
    response(409, {
      error: {
        code: "invalid_state",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "approved",
      replayed: false,
    }),
  ];
  const product = driver(async (url, options) => {
    calls.push({ url, options });
    return responses.shift();
  });
  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    (error) => error.code === "invalid_state" && error.requestId === "request-1"
  );
  assert.equal(calls.length, 2);
});


test("invalid apply state classifies every non-applied lifecycle state as terminal", async () => {
  for (const state of [
    "pending_approval",
    "approved",
    "rejected",
    "expired",
    "superseded",
    "withdrawn",
  ]) {
    let calls = 0;
    const product = driver(async () => {
      calls += 1;
      if (calls === 1) {
        return response(409, {
          error: {
            code: "invalid_state",
            request_id: `request-${state}`,
            retryable: false,
          },
        });
      }
      return response(200, {
        installation_id: "installation-1",
        promotion_id: DIGEST,
        revision: 2,
        state,
        payload_digest: DIGEST,
        apply_source_revision: null,
        replayed: false,
      });
    });
    await assert.rejects(
      product.applyWithDrainHandshake({
        installationId: "installation-1",
        promotionId: DIGEST,
        expectedPayloadDigest: DIGEST,
        expectedRevision: 2,
        runtimeDrainAttempts: 3,
        runtimeDrainIntervalMilliseconds: 100,
      }),
      (error) => error.code === "invalid_state" && error.requestId === `request-${state}`
    );
    assert.equal(calls, 2);
  }
});


test("invalid apply state fails closed on an unknown lifecycle state", async () => {
  const responses = [
    response(409, {
      error: {
        code: "invalid_state",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "unknown",
      payload_digest: DIGEST,
      apply_source_revision: null,
      replayed: false,
    }),
  ];
  const product = driver(async () => responses.shift());
  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    /apply_status_state_invalid/
  );
});


test("invalid apply state does not retry a dependency failure from status observation", async () => {
  const calls = [];
  let sleeps = 0;
  const responses = [
    response(409, {
      error: {
        code: "invalid_state",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(503, {
      error: {
        code: "dependency_unavailable",
        request_id: "request-2",
        retryable: true,
      },
    }),
  ];
  const product = driver(async (url, options) => {
    calls.push({ url, options });
    return responses.shift();
  }, undefined, {
    sleep: async () => {
      sleeps += 1;
    },
  });
  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    (error) => error.status === 503 && error.code === "dependency_unavailable"
  );
  assert.equal(calls.length, 2);
  assert.equal(calls.filter((call) => call.options.method === "POST").length, 1);
  assert.equal(calls.filter((call) => call.options.method === "GET").length, 1);
  assert.equal(sleeps, 0);
});


test("invalid apply state fails closed on a foreign promotion observation", async () => {
  const responses = [
    response(409, {
      error: {
        code: "invalid_state",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(200, {
      installation_id: "installation-2",
      promotion_id: DIGEST,
      revision: 3,
      state: "live",
      replayed: false,
    }),
  ];
  const product = driver(async () => responses.shift());
  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    /promotion_identity_mismatch/
  );
});


test("invalid apply state observation is bounded while applying remains unresolved", async () => {
  let calls = 0;
  let sleeps = 0;
  const product = driver(async () => {
    calls += 1;
    if (calls === 1) {
      return response(409, {
        error: {
          code: "invalid_state",
          request_id: "request-1",
          retryable: false,
        },
      });
    }
    return response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 3,
      state: "applying",
      payload_digest: DIGEST,
      apply_source_revision: 2,
      replayed: false,
    });
  }, undefined, {
    sleep: async () => {
      sleeps += 1;
    },
  });
  await assert.rejects(
    product.applyWithDrainHandshake({
      installationId: "installation-1",
      promotionId: DIGEST,
      expectedPayloadDigest: DIGEST,
      expectedRevision: 2,
      runtimeDrainAttempts: 3,
      runtimeDrainIntervalMilliseconds: 100,
    }),
    /apply_resolution_timeout/
  );
  assert.equal(calls, 4);
  assert.equal(sleeps, 2);
});


test("invalid apply state refuses a mismatched payload or source revision", async () => {
  for (const mismatch of [
    { payload_digest: "b".repeat(64), apply_source_revision: 2 },
    { payload_digest: DIGEST, apply_source_revision: 1 },
  ]) {
    const responses = [
      response(409, {
        error: {
          code: "invalid_state",
          request_id: "request-1",
          retryable: false,
        },
      }),
      response(200, {
        installation_id: "installation-1",
        promotion_id: DIGEST,
        revision: 4,
        state: "runtime_pending",
        payload_digest: mismatch.payload_digest,
        apply_source_revision: mismatch.apply_source_revision,
        replayed: false,
      }),
    ];
    const product = driver(async () => responses.shift());
    await assert.rejects(
      product.applyWithDrainHandshake({
        installationId: "installation-1",
        promotionId: DIGEST,
        expectedPayloadDigest: DIGEST,
        expectedRevision: 2,
        runtimeDrainAttempts: 3,
        runtimeDrainIntervalMilliseconds: 100,
      }),
      /apply_resume_binding_mismatch/
    );
  }
});


test("one-shot flow repairs only an invalid working draft candidate once", async () => {
  const calls = [];
  const responses = [
    response(201, {
      session_id: "session-1",
      generation: 1,
      disposition: "created",
      projection: {
        state: "preview_ready",
        preview: { revision: 22, receipt: { candidate_ruleset_hash: DIGEST } },
      },
    }),
    response(422, {
      error: {
        code: "invalid_server_candidate",
        request_id: "request-1",
        retryable: false,
      },
    }),
    response(201, {
      session_id: "session-1",
      generation: 2,
      disposition: "created",
      projection: {
        state: "preview_ready",
        preview: { revision: 22, receipt: { candidate_ruleset_hash: DIGEST } },
      },
    }),
    response(201, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      summary: {
        panels: 1,
        modals: 1,
        rules: 4,
        actions: 15,
        target_version: 1,
        required_approvals: 1,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "approved",
      replayed: false,
    }),
    response(202, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "runtime_pending",
      replayed: false,
    }),
  ];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options, body: options.body && JSON.parse(options.body) });
    return responses.shift();
  };
  const evidence = await driver(fetchImpl).runOneShotProductFlow({
    installationId: "installation-1",
    sessionId: "session-1",
    message: "Create the private study room automation",
    confirmPreview: async () => true,
  });
  assert.equal(calls.length, 7);
  assert.deepEqual(calls[2].body.expected_generation, 1);
  assert.match(calls[2].body.message, /working_draft to validated_preview/);
  assert.deepEqual(calls[3].body, { expected_generation: 2 });
  assert.notEqual(
    calls[1].options.headers["idempotency-key"],
    calls[3].options.headers["idempotency-key"]
  );
  assert.equal(evidence.authoring.generation, 2);
  assert.equal(evidence.authoring.preview_revision, 22);
  assert.equal(evidence.applied.state, "runtime_pending");
  const serialized = JSON.stringify(evidence);
  assert.equal(serialized.includes("working_draft"), false);
  assert.equal(serialized.includes("Create the private study room automation"), false);
});


test("one-shot flow does not repair other promotion failures", async () => {
  const responses = [
    response(201, {
      session_id: "session-1",
      generation: 1,
      disposition: "created",
      projection: {
        state: "preview_ready",
        preview: { revision: 22, receipt: { candidate_ruleset_hash: DIGEST } },
      },
    }),
    response(422, {
      error: {
        code: "candidate_generation_stale",
        request_id: "request-1",
        retryable: false,
      },
    }),
  ];
  let calls = 0;
  await assert.rejects(
    driver(async () => {
      calls += 1;
      return responses.shift();
    }).runOneShotProductFlow({
      installationId: "installation-1",
      sessionId: "session-1",
      message: "Create the private study room automation",
      confirmPreview: async () => true,
    }),
    (error) => error.code === "candidate_generation_stale"
  );
  assert.equal(calls, 2);
});


test("one-shot flow requires an explicit preview confirmation boundary", async () => {
  let calls = 0;
  const product = driver(async () => {
    calls += 1;
    return response(500, null);
  });
  await assert.rejects(
    product.runOneShotProductFlow({
      installationId: "installation-1",
      sessionId: "session-1",
      message: "Create a study room",
    }),
    /preview_confirmation_required/
  );
  assert.equal(calls, 0);
});


test("mutation stops before fetch when the CSRF cookie is absent", async () => {
  let calls = 0;
  const product = driver(async () => {
    calls += 1;
    return response(500, null);
  }, "unrelated=value");
  await assert.rejects(
    product.authoringTurn({
      installationId: "installation-1",
      sessionId: "session-1",
      expectedGeneration: 0,
      message: "Create a study room",
    }),
    /csrf_cookie_missing/
  );
  assert.equal(calls, 0);
});


test("live polling requires product and operational live with a fresh lease", async () => {
  const responses = [
    response(200, { installation_id: "installation-1", promotion_id: DIGEST, state: "pending" }),
    response(200, { installation_id: "installation-1", promotion_id: DIGEST, state: "pending", runtime: { phase: "requested", serving: { state: "not_expected" } } }),
    response(200, { installation_id: "installation-1", promotion_id: DIGEST, state: "live" }),
    response(200, { installation_id: "installation-1", promotion_id: DIGEST, state: "live", runtime: { phase: "live", serving: { state: "fresh" } } }),
  ];
  const result = await driver(async () => responses.shift()).waitForLive({
    installationId: "installation-1",
    promotionId: DIGEST,
    attempts: 2,
    intervalMilliseconds: 100,
  });
  assert.equal(result.pending_observed, true);
  assert.equal(result.live_observed, true);
  assert.equal(result.attempts, 2);
  assert.equal(result.product_state, "live");
  assert.equal(result.runtime_phase, "live");
  assert.equal(Object.hasOwn(result, "product"), false);
  assert.equal(Object.hasOwn(result, "operational"), false);
});


test("live polling preserves a strict pending seed when the first poll is already live", async () => {
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: { phase: "live", serving: { state: "fresh" } },
    }),
  ];
  const product = driver(async () => responses.shift());
  const result = await product.waitForLive({
    installationId: "installation-1",
    promotionId: DIGEST,
    attempts: 1,
    intervalMilliseconds: 100,
    pendingObserved: true,
  });
  assert.equal(result.pending_observed, true);
  assert.equal(result.live_observed, true);
  assert.equal(result.attempts, 1);
  await assert.rejects(
    product.waitForLive({
      installationId: "installation-1",
      promotionId: DIGEST,
      attempts: 1,
      intervalMilliseconds: 100,
      pendingObserved: 1,
    }),
    /pending_observed_invalid/
  );
});


test("live restart confirmation cross-binds both canonical status projections", async () => {
  const heartbeat = "2026-08-03T01:00:01.000001Z";
  const expires = "2026-08-03T01:00:46.000001Z";
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      observed_at: "2026-08-03T01:00:02.000001Z",
      state: "live",
      attestation_revision: 11,
      last_serving_heartbeat: heartbeat,
      serving_lease_expires_at: expires,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        observed_at: "2026-08-03T01:00:02.000001Z",
        phase: "live",
        attestation: {
          deployment_revision: 11,
          convergence_attempt: 1,
          process_instance_id: PROCESS_INSTANCE_ID,
        },
        serving: {
          state: "fresh",
          last_heartbeat_at: heartbeat,
          lease_expires_at: expires,
        },
      },
    }),
  ];
  const confirmation = await driver(async () => responses.shift()).liveRuntimeRestartConfirmation({
    operationId: "d2:0123456789abcdef:certify-live-runtime-restart",
    installationId: "installation-1",
    promotionId: DIGEST,
    processInstanceId: PROCESS_INSTANCE_ID,
    shutdownBoundary: "2026-08-03T01:00:00Z",
  });
  assert.deepEqual(Object.keys(confirmation), [
    "schema_version",
    "kind",
    "checkpoint",
    "operation_id",
    "installation_id",
    "promotion_id",
    "public_origin",
    "shutdown_boundary",
    "observed_at",
    "product_state",
    "operational_state",
    "runtime_phase",
    "serving_state",
    "attestation_revision",
    "process_instance_id",
    "last_heartbeat_at",
    "lease_expires_at",
  ]);
  assert.equal(confirmation.attestation_revision, 11);
  assert.equal(confirmation.process_instance_id, PROCESS_INSTANCE_ID);
  assert.equal(confirmation.public_origin, "https://d2-api.starring.co.kr");
  assert.equal(confirmation.last_heartbeat_at, heartbeat);
  assert.equal(confirmation.lease_expires_at, expires);
});


test("live restart confirmation rejects projection witness drift", async () => {
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      attestation_revision: 11,
      last_serving_heartbeat: "2026-08-03T01:00:01Z",
      serving_lease_expires_at: "2026-08-03T01:00:46Z",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        observed_at: "2026-08-03T01:00:02Z",
        phase: "live",
        attestation: {
          deployment_revision: 12,
          process_instance_id: PROCESS_INSTANCE_ID,
        },
        serving: {
          state: "fresh",
          last_heartbeat_at: "2026-08-03T01:00:01Z",
          lease_expires_at: "2026-08-03T01:00:46Z",
        },
      },
    }),
  ];
  await assert.rejects(
    driver(async () => responses.shift()).liveRuntimeRestartConfirmation({
      operationId: "d2:0123456789abcdef:certify-live-runtime-restart",
      installationId: "installation-1",
      promotionId: DIGEST,
      processInstanceId: PROCESS_INSTANCE_ID,
      shutdownBoundary: "2026-08-03T01:00:00Z",
    }),
    /live_restart_confirmation_invalid/
  );
});


test("live restart confirmation rejects a foreign canonical process identity", async () => {
  const heartbeat = "2026-08-03T01:00:01Z";
  const expires = "2026-08-03T01:00:46Z";
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      attestation_revision: 11,
      last_serving_heartbeat: heartbeat,
      serving_lease_expires_at: expires,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        observed_at: "2026-08-03T01:00:02Z",
        phase: "live",
        attestation: {
          deployment_revision: 11,
          process_instance_id: "f".repeat(32),
        },
        serving: {
          state: "fresh",
          last_heartbeat_at: heartbeat,
          lease_expires_at: expires,
        },
      },
    }),
  ];
  await assert.rejects(
    driver(async () => responses.shift()).liveRuntimeRestartConfirmation({
      operationId: "d2:0123456789abcdef:certify-live-runtime-restart",
      installationId: "installation-1",
      promotionId: DIGEST,
      processInstanceId: PROCESS_INSTANCE_ID,
      shutdownBoundary: "2026-08-03T01:00:00Z",
    }),
    /live_restart_confirmation_invalid/
  );
});


test("live restart confirmation rejects a lease beyond the runtime contract", async () => {
  const heartbeat = "2026-08-03T01:00:01Z";
  const expires = "2026-08-03T01:00:47Z";
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      attestation_revision: 11,
      last_serving_heartbeat: heartbeat,
      serving_lease_expires_at: expires,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        observed_at: "2026-08-03T01:00:02Z",
        phase: "live",
        attestation: {
          deployment_revision: 11,
          process_instance_id: PROCESS_INSTANCE_ID,
        },
        serving: {
          state: "fresh",
          last_heartbeat_at: heartbeat,
          lease_expires_at: expires,
        },
      },
    }),
  ];
  await assert.rejects(
    driver(async () => responses.shift()).liveRuntimeRestartConfirmation({
      operationId: "d2:0123456789abcdef:certify-live-runtime-restart",
      installationId: "installation-1",
      promotionId: DIGEST,
      processInstanceId: PROCESS_INSTANCE_ID,
      shutdownBoundary: "2026-08-03T01:00:00Z",
    }),
    /live_restart_confirmation_invalid/
  );
});


test("one-shot flow rejects identity drift before the next mutation", async () => {
  const responses = [
    response(201, {
      session_id: "session-1",
      generation: 1,
      disposition: "created",
      projection: {
        state: "preview_ready",
        preview: { revision: 1, receipt: { candidate_ruleset_hash: DIGEST } },
      },
    }),
    response(201, {
      installation_id: "installation-other",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      replayed: false,
    }),
  ];
  let calls = 0;
  await assert.rejects(
    driver(async () => {
      calls += 1;
      return responses.shift();
    }).runOneShotProductFlow({
      installationId: "installation-1",
      sessionId: "session-1",
      message: "Create a study room",
      confirmPreview: async () => true,
    }),
    /promotion_identity_mismatch/
  );
  assert.equal(calls, 2);
});


test("problem responses expose only the closed status and public code", async () => {
  const product = driver(async () => response(503, {
    error: {
      code: "request_failed",
      message: "not retained",
      request_id: "request-1",
      retryable: true,
    },
  }));
  await assert.rejects(product.me(), (error) => {
    assert.equal(error.name, "StarringD2ProductRequestError");
    assert.equal(error.status, 503);
    assert.equal(error.code, "request_failed");
    assert.equal(error.retryable, true);
    assert.equal(error.message, "product_request_failed");
    return true;
  });
});


test("every fetch is aborted by the fixed request deadline", async () => {
  const started = Date.now();
  const product = driver(
    async (_url, options) => new Promise((_resolve, reject) => {
      options.signal.addEventListener(
        "abort",
        () => reject(options.signal.reason),
        { once: true }
      );
    }),
    "__Host-starring_csrf=csrf-value",
    { requestTimeoutMilliseconds: 10 }
  );
  await assert.rejects(product.me(), /product_request_timeout/);
  assert.ok(Date.now() - started < 1000);
});


test("authentication evidence is exact and excludes profile and session material", async () => {
  const calls = [];
  const responses = [
    response(200, {
      principal_id: "discord:1056857223529250906",
      display_name: "not retained",
    }),
    response(204, null),
  ];
  const evidence = await driver(async (url, options) => {
    calls.push({ url, options });
    return responses.shift();
  }, undefined, {
    now: () => "2026-08-04T11:00:00Z",
  }).authenticationEvidence({
    installationId: "installation-1",
    guildId: "1533137713476272288",
  });
  assert.equal(calls.length, 2);
  assert.deepEqual(
    Object.keys(evidence),
    [
      "schema_version",
      "kind",
      "observed_at",
      "public_origin",
      "me_status",
      "principal_id",
      "installation_id",
      "guild_id",
      "authority_check_status",
    ]
  );
  assert.equal(evidence.kind, "starring.d2.browser-authentication-evidence.v1");
  assert.equal(evidence.authority_check_status, 204);
  assert.equal(JSON.stringify(evidence).includes("not retained"), false);
  assert.equal(JSON.stringify(evidence).includes("csrf-value"), false);
});


test("live evidence binds both public deployment projections", async () => {
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "pending",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "pending",
      runtime: null,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        phase: "live",
        serving: { state: "fresh" },
      },
    }),
  ];
  const evidence = await driver(async () => responses.shift(), undefined, {
    now: () => "2026-08-04T11:01:00Z",
  }).waitForLiveEvidence({
    installationId: "installation-1",
    promotionId: DIGEST,
    attempts: 2,
    intervalMilliseconds: 100,
  });
  assert.equal(evidence.kind, "starring.d2.browser-live-evidence.v1");
  assert.equal(evidence.pending_observed, true);
  assert.equal(evidence.live_observed, true);
  assert.equal(evidence.attempts, 2);
  assert.equal(evidence.deployment_http_status, 200);
  assert.equal(evidence.operational_http_status, 200);
});


test("live loss evidence accepts only retryable public dependency failures", async () => {
  const sleeps = [];
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        phase: "live",
        serving: { state: "fresh" },
      },
    }),
    response(503, {
      error: {
        code: "dependency_unavailable",
        request_id: "request-product",
        retryable: true,
      },
    }),
    response(503, {
      error: {
        code: "dependency_unavailable",
        request_id: "request-operational",
        retryable: true,
      },
    }),
  ];
  const evidence = await driver(async () => responses.shift(), undefined, {
    sleep: async (milliseconds) => sleeps.push(milliseconds),
    now: () => "2026-08-04T11:02:00Z",
  }).waitForLiveLoss({
    installationId: "installation-1",
    promotionId: DIGEST,
    attempts: 2,
    intervalMilliseconds: 100,
  });
  assert.deepEqual(sleeps, [100]);
  assert.equal(evidence.kind, "starring.d2.browser-live-loss-evidence.v1");
  assert.equal(evidence.live_lost, true);
  assert.equal(evidence.deployment_http_status, 503);
  assert.equal(evidence.operational_http_status, 503);
  assert.equal(evidence.product_state, "unavailable");
  assert.equal(evidence.operational_state, "unavailable");
  assert.equal(evidence.runtime_phase, "unavailable");
  assert.equal(evidence.serving_state, "unavailable");
  assert.equal(evidence.public_code, "dependency_unavailable");
  assert.equal(evidence.retryable, true);
  assert.equal(Object.hasOwn(evidence, "request_id"), false);
});


test("live loss evidence replaces null runtime state with closed sentinels", async () => {
  const responses = [
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "pending",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "pending",
      runtime: null,
    }),
  ];
  const evidence = await driver(async () => responses.shift()).waitForLiveLoss({
    installationId: "installation-1",
    promotionId: DIGEST,
    attempts: 1,
    intervalMilliseconds: 100,
  });
  assert.equal(evidence.product_state, "pending");
  assert.equal(evidence.operational_state, "pending");
  assert.equal(evidence.runtime_phase, "unavailable");
  assert.equal(evidence.serving_state, "unavailable");
  assert.equal(evidence.public_code, "live_state_lost");
  assert.equal(evidence.retryable, false);
});


test("replacement evidence binds one reviewed target transition without retaining the prompt", async () => {
  const sourcePromotionId = "b".repeat(64);
  const responses = [
    response(201, {
      session_id: "session-replacement",
      generation: 1,
      disposition: "created",
      projection: {
        state: "preview_ready",
        preview: { revision: 1, receipt: { candidate_ruleset_hash: DIGEST } },
      },
    }),
    response(201, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 1,
      state: "pending_approval",
      payload_digest: DIGEST,
      summary: {
        panels: 1,
        modals: 1,
        rules: 4,
        actions: 15,
        target_version: 2,
        required_approvals: 1,
      },
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      revision: 2,
      state: "approved",
      replayed: false,
    }),
    response(202, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "runtime_pending",
      replayed: false,
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
    }),
    response(200, {
      installation_id: "installation-1",
      promotion_id: DIGEST,
      state: "live",
      runtime: {
        phase: "live",
        serving: { state: "fresh" },
      },
    }),
  ];
  const evidence = await driver(async () => responses.shift(), undefined, {
    now: () => "2026-08-04T11:03:00Z",
  }).runReplacementFlow({
    installationId: "installation-1",
    sourcePromotionId,
    replacementKind: "update",
    sessionId: "session-replacement",
    message: "Do not retain this replacement prompt",
    confirmPreview: async () => true,
    liveAttempts: 1,
    liveIntervalMilliseconds: 100,
  });
  assert.equal(evidence.kind, "starring.d2.browser-replacement-evidence.v1");
  assert.equal(evidence.source_promotion_id, sourcePromotionId);
  assert.equal(evidence.replacement_promotion_id, DIGEST);
  assert.equal(evidence.replacement_kind, "update");
  assert.equal(evidence.pending_observed, true);
  assert.equal(evidence.live_observed, true);
  assert.equal(JSON.stringify(evidence).includes("replacement prompt"), false);
});
