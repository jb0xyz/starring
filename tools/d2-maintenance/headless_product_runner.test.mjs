import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { executeHeadless } from "./headless_product_runner.mjs";


const ORIGIN = "https://d2-api.starring.co.kr";
const SESSION = Buffer.alloc(32, 0xa1).toString("base64url");
const CSRF = Buffer.alloc(32, 0xb2).toString("base64url");
const PRINCIPAL = "discord:1056857223529250906";
const GUILD = "1536845588954353676";
const INSTALLATION = "installation:starring-d2-test";
const RUN_ID = "d2-20260812t000000z-0123456789ab";
const MANIFEST_SHA256 = "1".repeat(64);
const SCENARIO_SHA256 = "6".repeat(64);
const AUTHORING_SESSION_PREFIX = "d2a-study-room-v1";
const AUTHORING_SESSION_ID = `${AUTHORING_SESSION_PREFIX}-0123456789abcdef`;
const CANDIDATE_HASH = "2".repeat(64);
const PAYLOAD_DIGEST = "3".repeat(64);
const TARGET_HASH = "4".repeat(64);
const PROMOTION_ID = "5".repeat(64);
const PROCESS_INSTANCE_ID = "0123456789abcdef0123456789abcdef";
const COOKIE = `__Host-starring_session=${SESSION}; __Host-starring_csrf=${CSRF}`;
const SUMMARY = Object.freeze({
  panels: 1,
  modals: 1,
  rules: 4,
  actions: 2,
  target_version: 1,
  required_approvals: 1,
});
const RUNNER_PATH = fileURLToPath(new URL("./headless_product_runner.mjs", import.meta.url));
const BASE_EVIDENCE_FIELDS = [
  "schema_version",
  "kind",
  "certification_class",
  "operation",
  "observed_at",
  "run_id",
  "manifest_sha256",
  "public_origin",
  "principal_id",
  "guild_id",
  "installation_id",
  "direct_auth_used",
  "release_eligible",
  "uncovered_release_boundaries",
  "logout_status",
  "post_logout_me_status",
];


function response(status, body, { redirected = false } = {}) {
  return {
    status,
    ok: status >= 200 && status < 300,
    redirected,
    text: async () => (body === null ? "" : JSON.stringify(body)),
  };
}


function scenario(overrides = {}) {
  return {
    schema_version: 1,
    kind: "starring.d2a.product-scenario.v1",
    session_id_prefix: AUTHORING_SESSION_PREFIX,
    message: "Create the bounded study-room automation.",
    expected_generation: 0,
    expected_summary: { ...SUMMARY },
    ...overrides,
  };
}


function input(operation = "auth-smoke", overrides = {}) {
  return {
    schema_version: 1,
    session: SESSION,
    csrf: CSRF,
    public_origin: ORIGIN,
    principal_id: PRINCIPAL,
    guild_id: GUILD,
    installation_id: INSTALLATION,
    run_id: RUN_ID,
    manifest_sha256: MANIFEST_SHA256,
    operation,
    ...(operation === "one-shot" ? {
      scenario: scenario(),
      scenario_sha256: SCENARIO_SHA256,
      authoring_session_id: AUTHORING_SESSION_ID,
    } : {}),
    ...overrides,
  };
}


function fakeProductServer(options = {}) {
  const requests = [];
  let loggedOut = false;
  let firstRequest = true;
  const fetchImpl = async (resource, init) => {
    const url = new URL(resource);
    const headers = Object.fromEntries(init.headers.entries());
    requests.push({ url, method: init.method, headers, redirect: init.redirect, body: init.body });
    if (options.throwSecret === true) {
      throw new Error(`transport ${SESSION} ${CSRF}`);
    }
    if (options.redirectFirst === true && firstRequest) {
      firstRequest = false;
      return response(200, { principal_id: PRINCIPAL }, { redirected: true });
    }
    firstRequest = false;
    if (url.pathname === "/v1/logout" && init.method === "POST") {
      loggedOut = true;
      return response(options.logoutStatus || 204, null);
    }
    if (url.pathname === "/v1/me") {
      if (loggedOut) {
        const status = options.postLogoutStatus || 401;
        return response(status, status === 200 ? { principal_id: PRINCIPAL } : {
          error: { code: "unauthenticated", retryable: false, request_id: "request-logout" },
        });
      }
      return response(200, { principal_id: PRINCIPAL, display_name: "test" });
    }
    if (url.pathname === `/v1/installations/${encodeURIComponent(INSTALLATION)}/authority-check`) {
      return response(204, null);
    }
    if (url.pathname.endsWith("/turns") && init.method === "POST") {
      const encodedSessionId = url.pathname.match(/\/authoring\/sessions\/([^/]+)\/turns$/)?.[1];
      return response(201, {
        session_id: decodeURIComponent(encodedSessionId),
        generation: 1,
        disposition: "accepted",
        projection: {
          state: "preview_ready",
          preview: {
            revision: 1,
            receipt: { candidate_ruleset_hash: CANDIDATE_HASH },
          },
          model_completions: 1,
        },
      });
    }
    if (url.pathname.endsWith("/promotions") && init.method === "POST") {
      return response(201, {
        installation_id: INSTALLATION,
        promotion_id: PROMOTION_ID,
        revision: 1,
        state: "pending_approval",
        replayed: false,
        payload_digest: PAYLOAD_DIGEST,
      });
    }
    if (url.pathname.endsWith("/approval-preview") && init.method === "GET") {
      return response(200, {
        installation_id: INSTALLATION,
        promotion_id: PROMOTION_ID,
        revision: 1,
        state: "pending_approval",
        payload_digest: PAYLOAD_DIGEST,
        summary: {
          ...SUMMARY,
          target_content_hash: TARGET_HASH,
        },
      });
    }
    if (url.pathname.endsWith("/approvals") && init.method === "POST") {
      return response(201, {
        installation_id: INSTALLATION,
        promotion_id: PROMOTION_ID,
        revision: 2,
        state: "approved",
        replayed: false,
      });
    }
    if (url.pathname.endsWith("/apply") && init.method === "POST") {
      return response(202, {
        installation_id: INSTALLATION,
        promotion_id: PROMOTION_ID,
        revision: 3,
        state: "runtime_pending",
        replayed: false,
      });
    }
    if (url.pathname.startsWith("/v1/") && url.pathname.endsWith("/deployment")) {
      if (options.terminalLive === false) {
        return response(200, {
          installation_id: INSTALLATION,
          promotion_id: PROMOTION_ID,
          state: "pending",
        });
      }
      return response(200, {
        installation_id: INSTALLATION,
        promotion_id: PROMOTION_ID,
        observed_at: "2026-08-12T00:00:05Z",
        state: "live",
        retryable: false,
        failure_code: null,
        attestation_revision: 11,
        last_serving_heartbeat: "2026-08-12T00:00:00Z",
        serving_lease_expires_at: "2026-08-12T00:00:45Z",
      });
    }
    if (url.pathname.startsWith("/v2/") && url.pathname.endsWith("/deployment")) {
      if (options.terminalLive === false) {
        return response(200, {
          installation_id: INSTALLATION,
          promotion_id: PROMOTION_ID,
          state: "pending",
          runtime: { phase: "starting", serving: { state: "pending" } },
        });
      }
      return response(200, {
        installation_id: INSTALLATION,
        promotion_id: PROMOTION_ID,
        decision_observed_at: "2026-08-12T00:00:08Z",
        state: "live",
        runtime: {
          observed_at: "2026-08-12T00:00:10Z",
          phase: "live",
          current_attempt: 1,
          last_failure_attempt: null,
          failure: null,
          retry: null,
          operator_action: null,
          attestation: {
            deployment_revision: 11,
            convergence_attempt: 1,
            process_instance_id: PROCESS_INSTANCE_ID,
          },
          serving: {
            state: "fresh",
            last_heartbeat_at: "2026-08-12T00:00:00Z",
            lease_expires_at: "2026-08-12T00:00:45Z",
          },
        },
      });
    }
    return response(404, {
      error: { code: "not_found", retryable: false, request_id: "request-missing" },
    });
  };
  return { fetchImpl, requests };
}


function dependencies(server) {
  return {
    fetchImpl: server.fetchImpl,
    now: () => "2026-08-12T00:00:12.000Z",
    randomUUID: () => "00000000-0000-4000-8000-000000000001",
    sleep: async () => {},
  };
}


test("auth smoke injects exact cookies, verifies authority, logs out, and emits only non-release D2A evidence", async () => {
  const server = fakeProductServer();
  const evidence = await executeHeadless(input(), dependencies(server));

  assert.equal(evidence.kind, "starring.d2a.authentication-evidence.v1");
  assert.equal(evidence.direct_auth_used, true);
  assert.equal(evidence.release_eligible, false);
  assert.equal(evidence.certification_class, "automated_maintenance_v1");
  assert.equal(evidence.me_status, 200);
  assert.equal(evidence.authority_check_status, 204);
  assert.equal(evidence.logout_status, 204);
  assert.equal(evidence.post_logout_me_status, 401);
  assert.deepEqual(
    Object.keys(evidence).sort(),
    [...BASE_EVIDENCE_FIELDS, "me_status", "authority_check_status"].sort(),
  );
  assert.deepEqual(evidence.uncovered_release_boundaries, [
    "discord_oauth_consent_and_code_exchange",
    "real_discord_gateway_interactions",
    "discord_web_confirmation_surface",
    "human_preview_approval",
    "disposable_guild_deletion",
  ]);
  assert.ok(!evidence.kind.includes("browser"));
  assert.ok(!evidence.kind.includes("chrome"));
  assert.ok(!JSON.stringify(evidence).includes(SESSION));
  assert.ok(!JSON.stringify(evidence).includes(CSRF));

  assert.equal(server.requests.length, 4);
  for (const request of server.requests) {
    assert.equal(request.headers.cookie, COOKIE);
    assert.equal(request.redirect, "error");
    if (request.method === "GET") {
      assert.equal(request.headers.origin, undefined);
    }
  }
  const logout = server.requests.find((request) => request.url.pathname === "/v1/logout");
  assert.equal(logout.headers.origin, ORIGIN);
  assert.equal(logout.headers["x-csrf-token"], CSRF);
});


test("one-shot uses the real product driver and emits hashes and statuses without browser evidence", async () => {
  const server = fakeProductServer();
  const evidence = await executeHeadless(input("one-shot"), dependencies(server));

  assert.equal(evidence.kind, "starring.d2a.one-shot-product-evidence.v1");
  assert.equal(evidence.release_eligible, false);
  assert.equal(evidence.direct_auth_used, true);
  assert.equal(evidence.scenario_sha256, SCENARIO_SHA256);
  assert.equal(evidence.candidate_ruleset_hash, CANDIDATE_HASH);
  assert.equal(evidence.payload_digest, PAYLOAD_DIGEST);
  assert.equal(evidence.target_content_hash, TARGET_HASH);
  assert.equal(evidence.promotion_id, PROMOTION_ID);
  assert.equal(Object.hasOwn(evidence, "me_status"), false);
  assert.equal(Object.hasOwn(evidence, "authority_check_status"), false);
  assert.equal(evidence.logout_status, 204);
  assert.equal(evidence.post_logout_me_status, 401);
  assert.equal(evidence.authoring_http_status, 201);
  assert.equal(evidence.promotion_http_status, 201);
  assert.equal(evidence.preview_http_status, 200);
  assert.equal(evidence.approval_http_status, 201);
  assert.equal(evidence.apply_http_status, 202);
  assert.deepEqual(evidence.summary, SUMMARY);
  assert.equal(evidence.deployment_http_status, 200);
  assert.equal(evidence.operational_http_status, 200);
  assert.equal(evidence.live_attempts, 1);
  assert.equal(evidence.pending_observed, true);
  assert.equal(evidence.live_observed, true);
  assert.equal(evidence.product_state, "live");
  assert.equal(evidence.operational_state, "live");
  assert.equal(evidence.runtime_phase, "live");
  assert.equal(evidence.serving_state, "fresh");
  assert.equal(evidence.process_instance_id, PROCESS_INSTANCE_ID);
  assert.deepEqual(
    Object.keys(evidence).sort(),
    [
      ...BASE_EVIDENCE_FIELDS,
      "scenario_sha256",
      "authoring_http_status",
      "promotion_http_status",
      "preview_http_status",
      "approval_http_status",
      "apply_http_status",
      "authoring_session_id",
      "authoring_generation",
      "promotion_id",
      "candidate_ruleset_hash",
      "target_content_hash",
      "payload_digest",
      "preview_state",
      "approval_state",
      "apply_state",
      "apply_attempts",
      "runtime_drain_observed",
      "runtime_pending_observed",
      "apply_resumed_after_conflict",
      "apply_status_observations",
      "summary",
      "live_observed_at",
      "deployment_http_status",
      "operational_http_status",
      "live_attempts",
      "pending_observed",
      "live_observed",
      "product_state",
      "operational_state",
      "runtime_phase",
      "serving_state",
      "deployment_observed_at",
      "deployment_attestation_revision",
      "deployment_last_heartbeat_at",
      "deployment_lease_expires_at",
      "decision_observed_at",
      "runtime_observed_at",
      "current_attempt",
      "attestation_revision",
      "convergence_attempt",
      "process_instance_id",
      "last_heartbeat_at",
      "lease_expires_at",
    ].sort(),
  );
  const serialized = JSON.stringify(evidence);
  assert.ok(!serialized.includes("starring.d2.browser"));
  assert.ok(!serialized.includes("starring.d2.chrome"));
  assert.ok(!serialized.includes("authoring_evidence"));
  assert.ok(!serialized.includes(SESSION));
  assert.ok(!serialized.includes(CSRF));

  const mutations = server.requests.filter((request) => request.method !== "GET");
  assert.ok(mutations.length >= 4);
  for (const request of mutations) {
    assert.equal(request.headers.cookie, COOKIE);
    assert.equal(request.headers.origin, ORIGIN);
    assert.equal(request.headers["x-csrf-token"], CSRF);
  }
  const terminalReads = server.requests.filter(
    (request) => request.url.pathname.endsWith("/deployment"),
  );
  assert.equal(terminalReads.length, 2);
  for (const request of terminalReads) {
    assert.equal(request.headers.cookie, COOKIE);
    assert.equal(request.headers.origin, undefined);
    assert.equal(request.redirect, "error");
  }
});


test("one-shot cannot pass on runtime_pending without terminal fresh live state", async () => {
  const server = fakeProductServer({ terminalLive: false });
  await assert.rejects(
    executeHeadless(input("one-shot"), dependencies(server)),
    (error) => error.code === "deployment_live_timeout",
  );
  assert.equal(
    server.requests.some((request) => request.url.pathname === "/v1/logout"),
    true,
  );
  assert.equal(server.requests.at(-1).url.pathname, "/v1/me");
});


test("each invocation accepts a distinct issuer-resolved authoring session id", async () => {
  const firstId = `${AUTHORING_SESSION_PREFIX}-0123456789abcdef`;
  const secondId = `${AUTHORING_SESSION_PREFIX}-fedcba9876543210`;
  const first = await executeHeadless(
    input("one-shot", { authoring_session_id: firstId }),
    dependencies(fakeProductServer()),
  );
  const second = await executeHeadless(
    input("one-shot", { authoring_session_id: secondId }),
    dependencies(fakeProductServer()),
  );
  assert.equal(first.authoring_session_id, firstId);
  assert.equal(second.authoring_session_id, secondId);
  assert.notEqual(first.authoring_session_id, second.authoring_session_id);
});


test("scenario mismatch refuses approval but still revokes and verifies the session", async () => {
  const server = fakeProductServer();
  const mismatched = input("one-shot", {
    scenario: scenario({
      expected_summary: { ...SUMMARY, rules: SUMMARY.rules + 1 },
    }),
  });
  await assert.rejects(
    executeHeadless(mismatched, dependencies(server)),
    (error) => error.code === "scenario_confirmation_mismatch",
  );
  assert.equal(
    server.requests.some((request) => request.url.pathname.endsWith("/approvals")),
    false,
  );
  assert.equal(
    server.requests.some((request) => request.url.pathname === "/v1/logout"),
    true,
  );
  assert.equal(server.requests.at(-1).url.pathname, "/v1/me");
});


test("redirected responses fail closed and the credentials are still revoked", async () => {
  const server = fakeProductServer({ redirectFirst: true });
  await assert.rejects(
    executeHeadless(input(), dependencies(server)),
    (error) => error.code === "redirect_or_response_invalid",
  );
  assert.equal(
    server.requests.some((request) => request.url.pathname === "/v1/logout"),
    true,
  );
});


test("logout is incomplete unless the same credential is rejected by /v1/me", async () => {
  const server = fakeProductServer({ postLogoutStatus: 200 });
  await assert.rejects(
    executeHeadless(input(), dependencies(server)),
    (error) => error.code === "post_logout_session_active",
  );
});


test("transport exceptions and CLI validation never disclose session or CSRF secrets", async () => {
  const server = fakeProductServer({ throwSecret: true });
  let observed;
  try {
    await executeHeadless(input(), dependencies(server));
  } catch (error) {
    observed = error;
  }
  assert.equal(observed.code, "network_request_failed");
  assert.ok(!String(observed).includes(SESSION));
  assert.ok(!String(observed.stack).includes(SESSION));
  assert.ok(!String(observed.stack).includes(CSRF));

  const invalid = input("auth-smoke", { operation: "forbidden-operation" });
  const completed = spawnSync(process.execPath, [RUNNER_PATH], {
    input: JSON.stringify(invalid),
    encoding: "utf8",
  });
  assert.equal(completed.status, 1);
  assert.equal(completed.stderr, "");
  assert.ok(!completed.stdout.includes(SESSION));
  assert.ok(!completed.stdout.includes(CSRF));
  assert.deepEqual(JSON.parse(completed.stdout), {
    schema_version: 1,
    kind: "starring.d2a.runner-error.v1",
    ok: false,
    error_code: "operation_invalid",
  });
});


test("stdin accepts one bounded JSON object, not concatenated objects", () => {
  const completed = spawnSync(process.execPath, [RUNNER_PATH], {
    input: `${JSON.stringify(input())}${JSON.stringify(input())}`,
    encoding: "utf8",
  });
  assert.equal(completed.status, 1);
  assert.equal(completed.stderr, "");
  assert.equal(JSON.parse(completed.stdout).error_code, "input_json_invalid");
});
