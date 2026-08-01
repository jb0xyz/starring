((root) => {
  "use strict";

  const RESOURCE_ID = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
  const DIGEST = /^[0-9a-f]{64}$/;
  const COOKIE_NAME = "__Host-starring_csrf";

  function requireResourceId(value, label) {
    if (typeof value !== "string" || !RESOURCE_ID.test(value)) {
      throw new Error(`${label}_invalid`);
    }
    return value;
  }

  function requireDigest(value, label) {
    if (typeof value !== "string" || !DIGEST.test(value)) {
      throw new Error(`${label}_invalid`);
    }
    return value;
  }

  function requireGeneration(value, label, allowZero) {
    const minimum = allowZero ? 0 : 1;
    if (!Number.isSafeInteger(value) || value < minimum) {
      throw new Error(`${label}_invalid`);
    }
    return value;
  }

  function normalizeOrigin(value) {
    const parsed = new URL(value);
    if (
      parsed.protocol !== "https:" ||
      parsed.username !== "" ||
      parsed.password !== "" ||
      (parsed.pathname !== "" && parsed.pathname !== "/") ||
      parsed.search !== "" ||
      parsed.hash !== ""
    ) {
      throw new Error("origin_invalid");
    }
    return parsed.origin;
  }

  function readCookie(cookieSource, name) {
    const raw = cookieSource();
    if (typeof raw !== "string") {
      throw new Error("cookie_source_invalid");
    }
    for (const component of raw.split(";")) {
      const candidate = component.trim();
      const separator = candidate.indexOf("=");
      if (separator > 0 && candidate.slice(0, separator) === name) {
        const value = candidate.slice(separator + 1);
        if (value.length > 0 && value.length <= 512) {
          return value;
        }
      }
    }
    throw new Error("csrf_cookie_missing");
  }

  function safeProblem(status, body) {
    const problem = body && typeof body === "object" ? body : {};
    const error = problem.error && typeof problem.error === "object" ? problem.error : {};
    const result = new Error("product_request_failed");
    result.name = "StarringD2ProductRequestError";
    result.status = status;
    result.code = typeof error.code === "string" ? error.code : "response_invalid";
    result.retryable = error.retryable === true;
    result.requestId = typeof error.request_id === "string" ? error.request_id : null;
    return result;
  }

  function projectionEvidence(turn) {
    const projection = turn.projection;
    const preview = projection && projection.preview;
    const receipt = preview && preview.receipt;
    return {
      session_id: turn.session_id,
      generation: turn.generation,
      disposition: turn.disposition,
      projection_state: projection && projection.state,
      preview_revision: preview && preview.revision,
      candidate_ruleset_hash: receipt && receipt.candidate_ruleset_hash,
    };
  }

  function decisionEvidence(view) {
    return {
      installation_id: view.installation_id,
      promotion_id: view.promotion_id,
      revision: view.revision,
      state: view.state,
      replayed: view.replayed,
    };
  }

  function requireBody(value, label) {
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new Error(`${label}_invalid`);
    }
    return value;
  }

  function requireScopedIdentity(value, installationId, promotionId, label) {
    const body = requireBody(value, label);
    if (
      body.installation_id !== installationId ||
      body.promotion_id !== promotionId
    ) {
      throw new Error(`${label}_identity_mismatch`);
    }
    return body;
  }

  function summaryEvidence(value) {
    const summary = requireBody(value, "approval_summary");
    const fields = ["panels", "modals", "rules", "actions"];
    for (const field of fields) {
      if (!Number.isSafeInteger(summary[field]) || summary[field] < 0) {
        throw new Error("approval_summary_invalid");
      }
    }
    if (
      !Number.isSafeInteger(summary.target_version) ||
      summary.target_version < 1 ||
      summary.required_approvals !== 1
    ) {
      throw new Error("approval_summary_invalid");
    }
    return Object.freeze({
      panels: summary.panels,
      modals: summary.modals,
      rules: summary.rules,
      actions: summary.actions,
      target_version: summary.target_version,
      required_approvals: summary.required_approvals,
    });
  }

  function createD2ProductDriver(options = {}) {
    const locationOrigin = root.location && root.location.origin;
    const origin = normalizeOrigin(options.origin || locationOrigin);
    if (locationOrigin && normalizeOrigin(locationOrigin) !== origin) {
      throw new Error("origin_mismatch");
    }
    const fetchImpl = options.fetchImpl || root.fetch;
    const cookieSource = options.cookieSource || (() => root.document.cookie);
    const randomUUID = options.randomUUID || (() => root.crypto.randomUUID());
    const sleep = options.sleep || ((milliseconds) => new Promise((resolve) => root.setTimeout(resolve, milliseconds)));
    const requestTimeoutMilliseconds = options.requestTimeoutMilliseconds === undefined
      ? 15000
      : options.requestTimeoutMilliseconds;
    if (
      typeof fetchImpl !== "function" ||
      typeof cookieSource !== "function" ||
      typeof randomUUID !== "function" ||
      !Number.isInteger(requestTimeoutMilliseconds) ||
      requestTimeoutMilliseconds < 10 ||
      requestTimeoutMilliseconds > 30000
    ) {
      throw new Error("driver_dependency_invalid");
    }

    function boundedSignal(callerSignal) {
      const controller = new root.AbortController();
      const forwardAbort = () => controller.abort(callerSignal.reason);
      if (callerSignal && callerSignal.aborted) {
        forwardAbort();
      } else if (callerSignal && typeof callerSignal.addEventListener === "function") {
        callerSignal.addEventListener("abort", forwardAbort, { once: true });
      }
      const timer = root.setTimeout(
        () => controller.abort(new Error("product_request_timeout")),
        requestTimeoutMilliseconds
      );
      return {
        signal: controller.signal,
        cleanup: () => {
          root.clearTimeout(timer);
          if (callerSignal && typeof callerSignal.removeEventListener === "function") {
            callerSignal.removeEventListener("abort", forwardAbort);
          }
        },
      };
    }

    function idempotencyKey(scope) {
      const normalizedScope = requireResourceId(scope, "idempotency_scope");
      const key = `d2.${normalizedScope}.${randomUUID().replaceAll("-", "")}`;
      if (!RESOURCE_ID.test(key)) {
        throw new Error("idempotency_key_invalid");
      }
      return key;
    }

    async function request(path, options = {}) {
      if (typeof path !== "string" || (!path.startsWith("/v1/") && !path.startsWith("/v2/"))) {
        throw new Error("request_path_invalid");
      }
      const method = options.method || "GET";
      const headers = { accept: "application/json" };
      let body;
      if (method !== "GET") {
        headers["content-type"] = "application/json";
        headers["x-csrf-token"] = readCookie(cookieSource, COOKIE_NAME);
        headers["idempotency-key"] = options.idempotencyKey || idempotencyKey("mutation");
        body = JSON.stringify(options.body || {});
      }
      const bounded = boundedSignal(options.signal);
      let response;
      let raw;
      try {
        response = await fetchImpl(new URL(path, origin).href, {
          method,
          headers,
          body,
          credentials: "same-origin",
          cache: "no-store",
          redirect: "error",
          signal: bounded.signal,
        });
        raw = response.status === 204 ? "" : await response.text();
      } finally {
        bounded.cleanup();
      }
      if (raw.length > 512 * 1024) {
        throw new Error("response_too_large");
      }
      let parsed = null;
      if (raw !== "") {
        try {
          parsed = JSON.parse(raw);
        } catch {
          throw safeProblem(response.status, null);
        }
      }
      if (!response.ok) {
        throw safeProblem(response.status, parsed);
      }
      return { status: response.status, body: parsed };
    }

    function installationPath(installationId) {
      return encodeURIComponent(requireResourceId(installationId, "installation_id"));
    }

    function sessionPath(sessionId) {
      return encodeURIComponent(requireResourceId(sessionId, "session_id"));
    }

    function promotionPath(promotionId) {
      return encodeURIComponent(requireDigest(promotionId, "promotion_id"));
    }

    async function me() {
      return request("/v1/me");
    }

    async function authorityCheck(installationId) {
      return request(`/v1/installations/${installationPath(installationId)}/authority-check`);
    }

    async function authoringTurn(input) {
      const installation = installationPath(input.installationId);
      const session = sessionPath(input.sessionId);
      const expectedSessionId = requireResourceId(input.sessionId, "session_id");
      const expectedGeneration = requireGeneration(input.expectedGeneration, "expected_generation", true);
      if (typeof input.message !== "string" || input.message.length === 0 || input.message.length > 64 * 1024) {
        throw new Error("message_invalid");
      }
      const result = await request(`/v1/installations/${installation}/authoring/sessions/${session}/turns`, {
        method: "POST",
        idempotencyKey: input.idempotencyKey || idempotencyKey("authoring"),
        body: { expected_generation: expectedGeneration, message: input.message },
        signal: input.signal,
      });
      const body = requireBody(result.body, "authoring_turn");
      if (body.session_id !== expectedSessionId) {
        throw new Error("authoring_turn_identity_mismatch");
      }
      return result;
    }

    async function authoringSession(installationId, sessionId) {
      const expectedSessionId = requireResourceId(sessionId, "session_id");
      const result = await request(`/v1/installations/${installationPath(installationId)}/authoring/sessions/${sessionPath(sessionId)}`);
      const body = requireBody(result.body, "authoring_session");
      if (body.session_id !== expectedSessionId) {
        throw new Error("authoring_session_identity_mismatch");
      }
      return result;
    }

    async function promote(input) {
      const expectedInstallationId = requireResourceId(input.installationId, "installation_id");
      const generation = requireGeneration(input.expectedGeneration, "expected_generation", false);
      const result = await request(`/v1/installations/${installationPath(input.installationId)}/authoring/sessions/${sessionPath(input.sessionId)}/promotions`, {
        method: "POST",
        idempotencyKey: input.idempotencyKey || idempotencyKey("promote"),
        body: { expected_generation: generation },
      });
      const body = requireBody(result.body, "promotion");
      if (body.installation_id !== expectedInstallationId) {
        throw new Error("promotion_identity_mismatch");
      }
      requireDigest(body.promotion_id, "promotion_id");
      return result;
    }

    async function promotion(installationId, promotionId) {
      const expectedInstallationId = requireResourceId(installationId, "installation_id");
      const expectedPromotionId = requireDigest(promotionId, "promotion_id");
      const result = await request(`/v1/installations/${installationPath(installationId)}/promotions/${promotionPath(promotionId)}`);
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "promotion");
      return result;
    }

    async function approvalPreview(installationId, promotionId) {
      const expectedInstallationId = requireResourceId(installationId, "installation_id");
      const expectedPromotionId = requireDigest(promotionId, "promotion_id");
      const result = await request(`/v1/installations/${installationPath(installationId)}/promotions/${promotionPath(promotionId)}/approval-preview`);
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "approval_preview");
      return result;
    }

    async function approve(input) {
      const expectedInstallationId = requireResourceId(input.installationId, "installation_id");
      const expectedPromotionId = requireDigest(input.promotionId, "promotion_id");
      const result = await request(`/v1/installations/${installationPath(input.installationId)}/promotions/${promotionPath(input.promotionId)}/approvals`, {
        method: "POST",
        idempotencyKey: input.idempotencyKey || idempotencyKey("approve"),
        body: {
          expected_payload_digest: requireDigest(input.expectedPayloadDigest, "payload_digest"),
          expected_revision: requireGeneration(input.expectedRevision, "expected_revision", false),
        },
      });
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "approval");
      return result;
    }

    async function apply(input) {
      const expectedInstallationId = requireResourceId(input.installationId, "installation_id");
      const expectedPromotionId = requireDigest(input.promotionId, "promotion_id");
      const result = await request(`/v1/installations/${installationPath(input.installationId)}/promotions/${promotionPath(input.promotionId)}/apply`, {
        method: "POST",
        idempotencyKey: input.idempotencyKey || idempotencyKey("apply"),
        body: {
          expected_payload_digest: requireDigest(input.expectedPayloadDigest, "payload_digest"),
          expected_revision: requireGeneration(input.expectedRevision, "expected_revision", false),
        },
      });
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "apply");
      return result;
    }

    async function deployment(installationId, promotionId) {
      const expectedInstallationId = requireResourceId(installationId, "installation_id");
      const expectedPromotionId = requireDigest(promotionId, "promotion_id");
      const result = await request(`/v1/installations/${installationPath(installationId)}/promotions/${promotionPath(promotionId)}/deployment`);
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "deployment");
      return result;
    }

    async function operationalDeployment(installationId, promotionId) {
      const expectedInstallationId = requireResourceId(installationId, "installation_id");
      const expectedPromotionId = requireDigest(promotionId, "promotion_id");
      const result = await request(`/v2/installations/${installationPath(installationId)}/promotions/${promotionPath(promotionId)}/deployment`);
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "operational_deployment");
      return result;
    }

    async function runOneShotProductFlow(input) {
      if (typeof input.confirmPreview !== "function") {
        throw new Error("preview_confirmation_required");
      }
      const turn = await authoringTurn({
        installationId: input.installationId,
        sessionId: input.sessionId,
        expectedGeneration: input.expectedGeneration || 0,
        message: input.message,
        idempotencyKey: input.authoringIdempotencyKey,
        signal: input.signal,
      });
      if (
        !turn.body ||
        turn.body.projection.state !== "preview_ready" ||
        !Number.isSafeInteger(turn.body.generation) ||
        turn.body.generation < 1
      ) {
        throw new Error("authoring_not_preview_ready");
      }
      const promoted = await promote({
        installationId: input.installationId,
        sessionId: input.sessionId,
        expectedGeneration: turn.body.generation,
        idempotencyKey: input.promotionIdempotencyKey,
      });
      if (!promoted.body || promoted.body.state !== "pending_approval") {
        throw new Error("promotion_not_pending_approval");
      }
      const preview = await approvalPreview(input.installationId, promoted.body.promotion_id);
      if (!preview.body || preview.body.state !== "pending_approval") {
        throw new Error("approval_preview_not_pending");
      }
      const safeSummary = summaryEvidence(preview.body.summary);
      const confirmation = Object.freeze({
        installation_id: preview.body.installation_id,
        promotion_id: preview.body.promotion_id,
        revision: preview.body.revision,
        payload_digest: preview.body.payload_digest,
        summary: safeSummary,
      });
      if ((await input.confirmPreview(confirmation)) !== true) {
        throw new Error("preview_not_approved_by_operator");
      }
      const approved = await approve({
        installationId: input.installationId,
        promotionId: promoted.body.promotion_id,
        expectedPayloadDigest: preview.body.payload_digest,
        expectedRevision: preview.body.revision,
        idempotencyKey: input.approvalIdempotencyKey,
      });
      if (!approved.body || approved.body.state !== "approved") {
        throw new Error("promotion_not_approved");
      }
      const applied = await apply({
        installationId: input.installationId,
        promotionId: promoted.body.promotion_id,
        expectedPayloadDigest: preview.body.payload_digest,
        expectedRevision: approved.body.revision,
        idempotencyKey: input.applyIdempotencyKey,
      });
      if (!applied.body || !["runtime_pending", "live"].includes(applied.body.state)) {
        throw new Error("promotion_not_applied");
      }
      return {
        authoring_http_status: turn.status,
        authoring: projectionEvidence(turn.body),
        promotion_http_status: promoted.status,
        promotion: decisionEvidence(promoted.body),
        preview_http_status: preview.status,
        preview: {
          installation_id: preview.body.installation_id,
          promotion_id: preview.body.promotion_id,
          revision: preview.body.revision,
          state: preview.body.state,
          payload_digest: preview.body.payload_digest,
          summary: safeSummary,
        },
        approval_http_status: approved.status,
        approval: decisionEvidence(approved.body),
        apply_http_status: applied.status,
        applied: {
          installation_id: applied.body.installation_id,
          promotion_id: applied.body.promotion_id,
          state: applied.body.state,
          replayed: applied.body.replayed,
        },
      };
    }

    async function waitForLive(input) {
      const attempts = input.attempts === undefined ? 60 : input.attempts;
      const intervalMilliseconds = input.intervalMilliseconds === undefined ? 2000 : input.intervalMilliseconds;
      if (!Number.isInteger(attempts) || attempts < 1 || attempts > 180) {
        throw new Error("attempts_invalid");
      }
      if (!Number.isInteger(intervalMilliseconds) || intervalMilliseconds < 100 || intervalMilliseconds > 10000) {
        throw new Error("poll_interval_invalid");
      }
      let pendingObserved = false;
      for (let attempt = 1; attempt <= attempts; attempt += 1) {
        const product = await deployment(input.installationId, input.promotionId);
        const operational = await operationalDeployment(input.installationId, input.promotionId);
        pendingObserved = pendingObserved || product.body.state === "pending";
        if (product.body.state === "failed" || operational.body.state === "failed") {
          throw new Error("deployment_failed");
        }
        if (
          product.body.state === "live" &&
          operational.body.state === "live" &&
          operational.body.runtime &&
          operational.body.runtime.phase === "live" &&
          operational.body.runtime.serving.state === "fresh"
        ) {
          return {
            pending_observed: pendingObserved,
            live_observed: true,
            attempts: attempt,
            installation_id: requireResourceId(input.installationId, "installation_id"),
            promotion_id: requireDigest(input.promotionId, "promotion_id"),
            product_state: product.body.state,
            operational_state: operational.body.state,
            runtime_phase: operational.body.runtime.phase,
            serving_state: operational.body.runtime.serving.state,
          };
        }
        if (attempt < attempts) {
          await sleep(intervalMilliseconds);
        }
      }
      throw new Error("deployment_live_timeout");
    }

    return Object.freeze({
      me,
      authorityCheck,
      authoringTurn,
      authoringSession,
      promote,
      promotion,
      approvalPreview,
      approve,
      apply,
      deployment,
      operationalDeployment,
      runOneShotProductFlow,
      waitForLive,
    });
  }

  Object.defineProperty(root, "StarringD2ProductDriver", {
    value: Object.freeze({ create: createD2ProductDriver }),
    configurable: false,
    enumerable: true,
    writable: false,
  });
})(globalThis);
