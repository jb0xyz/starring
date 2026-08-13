const SESSION_COOKIE = "__Host-starring_session";
const CSRF_COOKIE = "__Host-starring_csrf";

export class ApiError extends Error {
  constructor(status, code, retryable) {
    super(code || `http_${status}`);
    this.name = "ApiError";
    this.status = status;
    this.code = code || `http_${status}`;
    this.retryable = Boolean(retryable);
  }
}

export function cookieValue(cookieHeader, name) {
  for (const segment of cookieHeader.split(";")) {
    const separator = segment.indexOf("=");
    if (separator < 0) continue;
    const key = segment.slice(0, separator).trim();
    if (key !== name) continue;
    try {
      return decodeURIComponent(segment.slice(separator + 1));
    } catch {
      return null;
    }
  }
  return null;
}

export function operationKey(scope) {
  const normalized = String(scope).replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 48);
  return `${normalized}_${crypto.randomUUID()}`;
}

export class StarringApi {
  constructor(fetchImpl = globalThis.fetch) {
    this.fetchImpl = fetchImpl;
    this.operationKeys = new Map();
  }

  async request(path, options = {}) {
    if (!path.startsWith("/") || path.startsWith("//")) throw new TypeError("same_origin_path_required");
    const method = options.method || "GET";
    const headers = new Headers({ Accept: "application/json" });
    if (options.body !== undefined) {
      headers.set("content-type", "application/json");
      const csrf = cookieValue(document.cookie, CSRF_COOKIE);
      if (!csrf) throw new ApiError(401, "session_required", false);
      headers.set("x-csrf-token", csrf);
      if (options.idempotencyScope) {
        let key = this.operationKeys.get(options.idempotencyScope);
        if (!key) {
          key = operationKey(options.idempotencyScope);
          this.operationKeys.set(options.idempotencyScope, key);
        }
        headers.set("idempotency-key", key);
      }
    }
    const response = await this.fetchImpl(path, {
      method,
      headers,
      body: options.body === undefined ? undefined : JSON.stringify(options.body),
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
    if (response.status === 204) return null;
    const contentType = response.headers.get("content-type") || "";
    const payload = contentType.includes("json") ? await response.json() : null;
    if (!response.ok) {
      throw new ApiError(
        response.status,
        payload?.error?.code,
        payload?.error?.retryable,
      );
    }
    return payload;
  }

  clearOperation(scope) {
    this.operationKeys.delete(scope);
  }

  me() { return this.request("/v1/me"); }
  logout() { return this.request("/v1/logout", { method: "POST", body: {} }); }

  descriptor(installationId) {
    return this.request(`/v1/installations/${encodeURIComponent(installationId)}/authoring/automation-spec/descriptor`);
  }

  authoringTurn(installationId, sessionId, expectedGeneration, message, operationId) {
    const scope = `turn_${installationId}_${sessionId}_${expectedGeneration}_${operationId}`;
    return this.request(
      `/v1/installations/${encodeURIComponent(installationId)}/authoring/sessions/${encodeURIComponent(sessionId)}/turns`,
      { method: "POST", body: { expected_generation: expectedGeneration, message }, idempotencyScope: scope },
    );
  }

  promote(installationId, sessionId, expectedGeneration) {
    return this.request(
      `/v1/installations/${encodeURIComponent(installationId)}/authoring/sessions/${encodeURIComponent(sessionId)}/promotions`,
      { method: "POST", body: { expected_generation: expectedGeneration }, idempotencyScope: `promote_${sessionId}_${expectedGeneration}` },
    );
  }

  approvalPreview(installationId, promotionId) {
    return this.request(`/v1/installations/${encodeURIComponent(installationId)}/promotions/${encodeURIComponent(promotionId)}/approval-preview`);
  }

  approve(installationId, promotionId, payloadDigest, revision) {
    return this.request(
      `/v1/installations/${encodeURIComponent(installationId)}/promotions/${encodeURIComponent(promotionId)}/approvals`,
      { method: "POST", body: { expected_payload_digest: payloadDigest, expected_revision: revision }, idempotencyScope: `approve_${promotionId}_${revision}` },
    );
  }

  apply(installationId, promotionId, payloadDigest, revision) {
    return this.request(
      `/v1/installations/${encodeURIComponent(installationId)}/promotions/${encodeURIComponent(promotionId)}/apply`,
      { method: "POST", body: { expected_payload_digest: payloadDigest, expected_revision: revision }, idempotencyScope: `apply_${promotionId}_${revision}` },
    );
  }

  deployment(installationId, promotionId) {
    return this.request(`/v1/installations/${encodeURIComponent(installationId)}/promotions/${encodeURIComponent(promotionId)}/deployment`);
  }
}

export function browserHasSession(cookieHeader) {
  return cookieHeader.split(";").some((segment) => segment.trim().startsWith(`${SESSION_COOKIE}=`));
}
