import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";


const SOURCE = await readFile(new URL("./product_driver.js", import.meta.url), "utf8");
const DIGEST = "a".repeat(64);


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
  const serialized = JSON.stringify(evidence);
  assert.equal(serialized.includes("Create the private study room automation"), false);
  assert.equal(serialized.includes("assistant_message"), false);
  assert.equal(serialized.includes("ruleset"), true);
  assert.equal(serialized.includes('"hidden"'), false);
  assert.equal(Object.hasOwn(evidence.preview, "ruleset"), false);
  assert.equal(Object.hasOwn(evidence.preview.summary, "target_content_hash"), false);
  assert.equal(Object.hasOwn(evidence.preview.summary, "binding_fingerprint"), false);
  assert.equal(Object.hasOwn(evidence.preview.summary, "expires_at"), false);
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


test("runtime drain handshake is bounded and does not retry generic conflicts", async () => {
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
        code: "invalid_state",
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
    (error) => error.code === "invalid_state"
  );
  assert.equal(genericCalls, 1);
  assert.equal(genericSleeps, 0);
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
