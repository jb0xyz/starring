import test from "node:test";
import assert from "node:assert/strict";
import { cookieValue, operationKey } from "./api.js";
import {
  captureFlowToken,
  invalidateApprovalFlow,
  isFlowTokenCurrent,
  loadAuthoringContext,
  persistAuthoringContext,
} from "./flow.js";
import { approvalSummaryModel, projectionModel, safeErrorMessage } from "./projection.js";

test("cookie extraction is exact and malformed encoding fails closed", () => {
  assert.equal(cookieValue("a=1; __Host-starring_csrf=token%2D1; b=2", "__Host-starring_csrf"), "token-1");
  assert.equal(cookieValue("x__Host-starring_csrf=wrong", "__Host-starring_csrf"), null);
  assert.equal(cookieValue("__Host-starring_csrf=%ZZ", "__Host-starring_csrf"), null);
});

test("operation keys are bounded opaque retry identities", () => {
  const key = operationKey("authoring/session 1");
  assert.match(key, /^authoring_session_1_[0-9a-f-]{36}$/);
  assert.ok(key.length < 100);
});

test("a new turn or context invalidates every stale approval handle", () => {
  const state = {
    flowEpoch: 3,
    installationId: "install-a",
    sessionId: "session-a",
    generation: 2,
    promotion: { promotion_id: "promotion-a" },
    approvalPreview: { payload_digest: "a".repeat(64) },
    approved: { revision: 4 },
  };
  const stale = captureFlowToken(state);
  invalidateApprovalFlow(state);
  assert.equal(isFlowTokenCurrent(state, stale), false);
  assert.equal(state.promotion, null);
  assert.equal(state.approvalPreview, null);
  assert.equal(state.approved, null);

  const current = captureFlowToken(state);
  assert.equal(isFlowTokenCurrent(state, current), true);
  state.generation += 1;
  assert.equal(isFlowTokenCurrent(state, current), false);
});

test("session and generation persist as one validated reload context", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
  const original = {
    installationId: "install-a",
    sessionId: "123e4567-e89b-12d3-a456-426614174000",
    generation: 7,
  };
  assert.equal(persistAuthoringContext(storage, original), true);
  assert.deepEqual(loadAuthoringContext(storage, "fresh-session"), original);

  const longest = `tenant.prod:${"x".repeat(116)}`;
  const bounded = {
    installationId: longest,
    sessionId: longest,
    generation: 8,
  };
  assert.equal(longest.length, 128);
  persistAuthoringContext(storage, bounded);
  assert.deepEqual(loadAuthoringContext(storage, "fresh-session"), bounded);

  values.set("starring.authoring.context.v1", JSON.stringify({
    schema_version: 1,
    installation_id: "install-a",
    session_id: "",
    generation: 7,
  }));
  assert.deepEqual(
    loadAuthoringContext(storage, "fresh-session"),
    { installationId: "", sessionId: "fresh-session", generation: 0 },
  );

  values.set("starring.authoring.context.v1", JSON.stringify({
    schema_version: 1,
    installation_id: `x${longest}`,
    session_id: "session-a",
    generation: 7,
  }));
  assert.deepEqual(
    loadAuthoringContext(storage, "fresh-session"),
    { installationId: "", sessionId: "fresh-session", generation: 0 },
  );
});

test("generic projection model never branches on a recipe identifier", () => {
  const model = projectionModel({
    schema_version: 1,
    state: "preview_ready",
    assistant_message: "Ready",
    capabilities: [],
    draft: { panels: 2, modals: 1, rules: 3, actions: 4, unresolved_references: [] },
    preview: { draft: { panels: 2, modals: 1, rules: 3, actions: 4, unresolved_references: [] } },
  });
  assert.equal(model.previewReady, true);
  assert.equal(model.stateLabel, "미리보기 준비됨");
  assert.deepEqual(model.summary.map((item) => item.key), ["panels", "modals", "rules", "actions"]);
});

test("unknown projection states render a safe fallback", () => {
  const model = projectionModel({ schema_version: 1, state: "future_state", assistant_message: "Safe", draft: {} });
  assert.equal(model.stateLabel, "알 수 없는 상태");
  assert.equal(model.previewReady, false);
});

test("approval summaries and errors expose only closed display fields", () => {
  assert.deepEqual(approvalSummaryModel({ panels: 1, actions: 4, target_content_hash: "secret" }).map((item) => item.key), ["panels", "actions"]);
  assert.equal(safeErrorMessage({ code: "dependency_timeout" }), "외부 서비스 응답이 늦습니다. 잠시 후 다시 시도해 주세요.");
  assert.equal(safeErrorMessage({ code: "backend_secret_detail" }), "요청을 완료하지 못했습니다. 상태를 확인한 뒤 다시 시도해 주세요.");
});
