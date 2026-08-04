((root) => {
  "use strict";

  const RESOURCE_ID = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
  const DIGEST = /^[0-9a-f]{64}$/;
  const PROCESS_INSTANCE_ID = /^[0-9a-f]{32}$/;
  const UTC_TIMESTAMP = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.([0-9]{1,9}))?Z$/;
  const LIVE_RESTART_OPERATION = /^d2:[0-9a-f]{16}:certify-live-runtime-restart$/;
  const COOKIE_NAME = "__Host-starring_csrf";
  const LIVE_RESTART_CONFIRMATION_KIND = "starring.d2.live-runtime-restart-confirmation.v1";
  const AUTHENTICATION_EVIDENCE_KIND = "starring.d2.browser-authentication-evidence.v1";
  const AUTHORING_EVIDENCE_KIND = "starring.d2.browser-authoring-evidence.v1";
  const PREVIEW_READY_EVIDENCE_KIND = "starring.d2.browser-preview-ready-evidence.v1";
  const PRODUCT_DECISION_EVIDENCE_KIND = "starring.d2.browser-product-decision-evidence.v1";
  const CHROME_PREVIEW_CONFIRMATION_KIND = "starring.d2.chrome-preview-confirmation.v1";
  const CERTIFICATION_DECISION_COMMAND_KIND = "starring.d2.certification-decision-command.v1";
  const LIVE_EVIDENCE_KIND = "starring.d2.browser-live-evidence.v1";
  const LIVE_LOSS_EVIDENCE_KIND = "starring.d2.browser-live-loss-evidence.v1";
  const REPLACEMENT_EVIDENCE_KIND = "starring.d2.browser-replacement-evidence.v1";
  const DISCORD_INTERACTION_OBSERVATION_KIND = "starring.d2.browser-discord-interaction-observation.v1";
  const LIVE_FRESH_LEASE_CHECKPOINT = "live_fresh_lease";
  const SERVING_LEASE_MAXIMUM_NANOSECONDS = 45 * 1000000000;
  const APPLY_ACTIVE_STATES = new Set(["applying"]);
  const APPLY_COMPLETE_STATES = new Set(["runtime_pending", "live"]);
  const APPLY_TERMINAL_STATES = new Set([
    "pending_approval",
    "approved",
    "rejected",
    "expired",
    "superseded",
    "withdrawn",
  ]);
  const FINALIZE_EXISTING_PREVIEW_MESSAGE = "Do not change the current Draft. Keep every existing feature and the current community_hub channel binding exactly unchanged. The only allowed semantic transition is requested_outcome from working_draft to validated_preview. Revalidate the exact current candidate, run the required simulation, and finish with a promotable preview. Do not ask another question.";

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

  function requireSnowflake(value, label) {
    if (typeof value !== "string" || !/^[1-9][0-9]{0,19}$/.test(value)) {
      throw new Error(`${label}_invalid`);
    }
    return value;
  }

  function requireSnowflakeList(value, label) {
    if (!Array.isArray(value) || value.length < 1 || value.length > 128) {
      throw new Error(`${label}_invalid`);
    }
    const normalized = value.map((item) => requireSnowflake(item, label)).sort();
    if (new Set(normalized).size !== normalized.length) {
      throw new Error(`${label}_invalid`);
    }
    return normalized;
  }

  function requireGeneration(value, label, allowZero) {
    const minimum = allowZero ? 0 : 1;
    if (!Number.isSafeInteger(value) || value < minimum) {
      throw new Error(`${label}_invalid`);
    }
    return value;
  }

  function requireUtcTimestamp(value, label) {
    const match = typeof value === "string" ? UTC_TIMESTAMP.exec(value) : null;
    if (!match) {
      throw new Error(`${label}_invalid`);
    }
    const whole = value.replace(/\.[0-9]{1,9}Z$/, "Z");
    const milliseconds = Date.parse(whole);
    if (
      !Number.isFinite(milliseconds) ||
      new Date(milliseconds).toISOString().slice(0, 19) + "Z" !== whole
    ) {
      throw new Error(`${label}_invalid`);
    }
    return {
      value,
      seconds: milliseconds / 1000,
      nanoseconds: Number((match[1] || "").padEnd(9, "0")),
    };
  }

  function compareUtcTimestamps(left, right) {
    if (left.seconds !== right.seconds) {
      return left.seconds < right.seconds ? -1 : 1;
    }
    if (left.nanoseconds === right.nanoseconds) {
      return 0;
    }
    return left.nanoseconds < right.nanoseconds ? -1 : 1;
  }

  function utcDifferenceNanoseconds(later, earlier) {
    return (
      (later.seconds - earlier.seconds) * 1000000000 +
      later.nanoseconds -
      earlier.nanoseconds
    );
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

  function isRuntimeDrainConflict(error) {
    return Boolean(
      error &&
      error.name === "StarringD2ProductRequestError" &&
      error.status === 409 &&
      error.retryable === true &&
      ["runtime_drain_required", "runtime_drain_pending"].includes(error.code)
    );
  }

  function isInvalidStateConflict(error) {
    return Boolean(
      error &&
      error.name === "StarringD2ProductRequestError" &&
      error.status === 409 &&
      error.retryable === false &&
      error.code === "invalid_state"
    );
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
      model_completions: projection && projection.model_completions,
    };
  }

  function sessionProjectionEvidence(session) {
    const projection = session.projection;
    const preview = projection && projection.preview;
    const receipt = preview && preview.receipt;
    return {
      session_id: session.session_id,
      observed_generation: session.observed_generation,
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

  function requireApplyStatusBinding(body, command) {
    const increment = body.state === "applying" ? 1 : 2;
    if (
      body.payload_digest !== command.expectedPayloadDigest ||
      body.apply_source_revision !== command.expectedRevision ||
      !Number.isSafeInteger(body.revision) ||
      body.revision !== command.expectedRevision + increment
    ) {
      throw new Error("apply_resume_binding_mismatch");
    }
  }

  function classifyApplyStatus(body) {
    if (APPLY_ACTIVE_STATES.has(body.state)) {
      return "active";
    }
    if (APPLY_COMPLETE_STATES.has(body.state)) {
      return "complete";
    }
    if (APPLY_TERMINAL_STATES.has(body.state)) {
      return "terminal";
    }
    throw new Error("apply_status_state_invalid");
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

  function exactLiveProjectionEvidence(product, operational, evidenceObservedAt) {
    const productBody = requireBody(product.body, "deployment");
    const operationalBody = requireBody(operational.body, "operational_deployment");
    const runtime = requireBody(operationalBody.runtime, "operational_runtime");
    const attestation = requireBody(runtime.attestation, "operational_attestation");
    const serving = requireBody(runtime.serving, "operational_serving");
    const deploymentObservedAt = requireUtcTimestamp(
      productBody.observed_at,
      "deployment_observed_at",
    );
    const deploymentLastHeartbeatAt = requireUtcTimestamp(
      productBody.last_serving_heartbeat,
      "deployment_last_heartbeat_at",
    );
    const deploymentLeaseExpiresAt = requireUtcTimestamp(
      productBody.serving_lease_expires_at,
      "deployment_lease_expires_at",
    );
    const decisionObservedAt = requireUtcTimestamp(
      operationalBody.decision_observed_at,
      "decision_observed_at",
    );
    const runtimeObservedAt = requireUtcTimestamp(
      runtime.observed_at,
      "runtime_observed_at",
    );
    const lastHeartbeatAt = requireUtcTimestamp(
      serving.last_heartbeat_at,
      "last_heartbeat_at",
    );
    const leaseExpiresAt = requireUtcTimestamp(
      serving.lease_expires_at,
      "lease_expires_at",
    );
    const observed = requireUtcTimestamp(evidenceObservedAt, "observed_at");
    const deploymentRevision = requireGeneration(
      productBody.attestation_revision,
      "deployment_attestation_revision",
      false,
    );
    const attestationRevision = requireGeneration(
      attestation.deployment_revision,
      "attestation_revision",
      false,
    );
    const currentAttempt = requireGeneration(
      runtime.current_attempt,
      "current_attempt",
      false,
    );
    const convergenceAttempt = requireGeneration(
      attestation.convergence_attempt,
      "convergence_attempt",
      false,
    );
    const processInstanceId = typeof attestation.process_instance_id === "string"
      ? attestation.process_instance_id
      : "";
    if (
      productBody.state !== "live" ||
      operationalBody.state !== "live" ||
      runtime.phase !== "live" ||
      serving.state !== "fresh" ||
      !PROCESS_INSTANCE_ID.test(processInstanceId) ||
      deploymentRevision !== attestationRevision ||
      currentAttempt !== convergenceAttempt ||
      compareUtcTimestamps(deploymentLastHeartbeatAt, deploymentObservedAt) > 0 ||
      compareUtcTimestamps(deploymentObservedAt, deploymentLeaseExpiresAt) >= 0 ||
      compareUtcTimestamps(deploymentObservedAt, decisionObservedAt) > 0 ||
      compareUtcTimestamps(decisionObservedAt, runtimeObservedAt) > 0 ||
      compareUtcTimestamps(runtimeObservedAt, observed) > 0 ||
      compareUtcTimestamps(deploymentLastHeartbeatAt, lastHeartbeatAt) > 0 ||
      compareUtcTimestamps(deploymentLeaseExpiresAt, leaseExpiresAt) > 0 ||
      compareUtcTimestamps(lastHeartbeatAt, runtimeObservedAt) > 0 ||
      compareUtcTimestamps(runtimeObservedAt, leaseExpiresAt) >= 0 ||
      compareUtcTimestamps(observed, leaseExpiresAt) >= 0 ||
      utcDifferenceNanoseconds(deploymentLeaseExpiresAt, deploymentLastHeartbeatAt) <= 0 ||
      utcDifferenceNanoseconds(deploymentLeaseExpiresAt, deploymentLastHeartbeatAt) >
        SERVING_LEASE_MAXIMUM_NANOSECONDS ||
      utcDifferenceNanoseconds(leaseExpiresAt, lastHeartbeatAt) <= 0 ||
      utcDifferenceNanoseconds(leaseExpiresAt, lastHeartbeatAt) >
        SERVING_LEASE_MAXIMUM_NANOSECONDS
    ) {
      throw new Error("live_projection_identity_invalid");
    }
    return Object.freeze({
      deployment_observed_at: deploymentObservedAt.value,
      deployment_attestation_revision: deploymentRevision,
      deployment_last_heartbeat_at: deploymentLastHeartbeatAt.value,
      deployment_lease_expires_at: deploymentLeaseExpiresAt.value,
      decision_observed_at: decisionObservedAt.value,
      runtime_observed_at: runtimeObservedAt.value,
      current_attempt: currentAttempt,
      attestation_revision: attestationRevision,
      convergence_attempt: convergenceAttempt,
      process_instance_id: processInstanceId,
      last_heartbeat_at: lastHeartbeatAt.value,
      lease_expires_at: leaseExpiresAt.value,
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
    const now = options.now || (() => new Date().toISOString());
    const sleep = options.sleep || ((milliseconds) => new Promise((resolve) => root.setTimeout(resolve, milliseconds)));
    const requestTimeoutMilliseconds = options.requestTimeoutMilliseconds === undefined
      ? 15000
      : options.requestTimeoutMilliseconds;
    if (
      typeof fetchImpl !== "function" ||
      typeof cookieSource !== "function" ||
      typeof randomUUID !== "function" ||
      typeof now !== "function" ||
      !Number.isInteger(requestTimeoutMilliseconds) ||
      requestTimeoutMilliseconds < 10 ||
      requestTimeoutMilliseconds > 30000
    ) {
      throw new Error("driver_dependency_invalid");
    }

    function observedAt() {
      return requireUtcTimestamp(now(), "observed_at").value;
    }

    function discordInteractionObservation(input) {
      const roleIds = requireSnowflakeList(input.roleIds, "role_ids");
      const channelIds = requireSnowflakeList(input.channelIds, "channel_ids");
      const panelMessageIds = requireSnowflakeList(
        input.panelMessageIds,
        "panel_message_ids",
      );
      const resourceIds = [...roleIds, ...channelIds, ...panelMessageIds];
      if (
        new Set(resourceIds).size !== resourceIds.length ||
        input.createResponseObserved !== true ||
        input.joinResponseObserved !== true ||
        input.privateChannelObserved !== true ||
        input.roleAssignmentObserved !== true ||
        input.joinPanelObserved !== true
      ) {
        throw new Error("discord_interaction_observation_invalid");
      }
      const createInteractionId = requireSnowflake(
        input.createInteractionId,
        "create_interaction_id",
      );
      const joinInteractionId = requireSnowflake(
        input.joinInteractionId,
        "join_interaction_id",
      );
      if (createInteractionId === joinInteractionId) {
        throw new Error("discord_interaction_identity_invalid");
      }
      const actorUserId = requireSnowflake(input.actorUserId, "actor_user_id");
      const joinedRoleId = requireSnowflake(input.joinedRoleId, "joined_role_id");
      if (!roleIds.includes(joinedRoleId)) {
        throw new Error("discord_joined_role_invalid");
      }
      return Object.freeze({
        schema_version: 1,
        kind: DISCORD_INTERACTION_OBSERVATION_KIND,
        observed_at: observedAt(),
        guild_id: requireSnowflake(input.guildId, "guild_id"),
        resource_prefix: requireResourceId(input.resourcePrefix, "resource_prefix"),
        actor_user_id: actorUserId,
        create_interaction_id: createInteractionId,
        join_interaction_id: joinInteractionId,
        joined_role_id: joinedRoleId,
        role_ids: roleIds,
        channel_ids: channelIds,
        panel_message_ids: panelMessageIds,
        create_response_observed: true,
        join_response_observed: true,
        private_channel_observed: true,
        role_assignment_observed: true,
        join_panel_observed: true,
        confirmation_surface: "chrome_discord_web",
      });
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

    function requireCertificationDecisionCommand(value) {
      const command = requireBody(value, "certification_decision_command");
      const fields = [
        "schema_version",
        "kind",
        "installation_id",
        "authoring_session_id",
        "authoring_generation",
        "candidate_ruleset_hash",
        "preview_completion_challenge_sha256",
        "promotion_idempotency_key",
        "approval_idempotency_key",
        "apply_idempotency_key",
      ];
      if (
        Object.keys(command).length !== fields.length ||
        fields.some((field) => !Object.hasOwn(command, field)) ||
        command.schema_version !== 1 ||
        command.kind !== CERTIFICATION_DECISION_COMMAND_KIND
      ) {
        throw new Error("certification_decision_command_invalid");
      }
      const normalized = {
        schema_version: 1,
        kind: CERTIFICATION_DECISION_COMMAND_KIND,
        installation_id: requireResourceId(command.installation_id, "installation_id"),
        authoring_session_id: requireResourceId(
          command.authoring_session_id,
          "authoring_session_id",
        ),
        authoring_generation: requireGeneration(
          command.authoring_generation,
          "authoring_generation",
          false,
        ),
        candidate_ruleset_hash: requireDigest(
          command.candidate_ruleset_hash,
          "candidate_ruleset_hash",
        ),
        preview_completion_challenge_sha256: requireDigest(
          command.preview_completion_challenge_sha256,
          "preview_completion_challenge_sha256",
        ),
        promotion_idempotency_key: requireResourceId(
          command.promotion_idempotency_key,
          "promotion_idempotency_key",
        ),
        approval_idempotency_key: requireResourceId(
          command.approval_idempotency_key,
          "approval_idempotency_key",
        ),
        apply_idempotency_key: requireResourceId(
          command.apply_idempotency_key,
          "apply_idempotency_key",
        ),
      };
      if (
        new Set([
          normalized.promotion_idempotency_key,
          normalized.approval_idempotency_key,
          normalized.apply_idempotency_key,
        ]).size !== 3
      ) {
        throw new Error("certification_decision_command_key_reuse");
      }
      return Object.freeze(normalized);
    }

    function createCertificationDecisionCommand(input) {
      return requireCertificationDecisionCommand({
        schema_version: 1,
        kind: CERTIFICATION_DECISION_COMMAND_KIND,
        installation_id: input.installationId,
        authoring_session_id: input.sessionId,
        authoring_generation: input.authoringGeneration,
        candidate_ruleset_hash: input.candidateRulesetHash,
        preview_completion_challenge_sha256:
          input.previewCompletionChallengeSha256,
        promotion_idempotency_key: idempotencyKey("certification_promote"),
        approval_idempotency_key: idempotencyKey("certification_approve"),
        apply_idempotency_key: idempotencyKey("certification_apply"),
      });
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

    async function authenticationEvidence(input) {
      const installationId = requireResourceId(input.installationId, "installation_id");
      const guildId = requireSnowflake(input.guildId, "guild_id");
      const identity = await me();
      const body = requireBody(identity.body, "me");
      if (
        identity.status !== 200 ||
        typeof body.principal_id !== "string" ||
        !/^discord:[1-9][0-9]{0,19}$/.test(body.principal_id)
      ) {
        throw new Error("me_identity_invalid");
      }
      const authority = await authorityCheck(installationId);
      if (authority.status !== 204 || authority.body !== null) {
        throw new Error("authority_check_invalid");
      }
      return Object.freeze({
        schema_version: 1,
        kind: AUTHENTICATION_EVIDENCE_KIND,
        observed_at: observedAt(),
        public_origin: origin,
        me_status: identity.status,
        principal_id: body.principal_id,
        installation_id: installationId,
        guild_id: guildId,
        authority_check_status: authority.status,
      });
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

    async function promotion(installationId, promotionId, signal) {
      const expectedInstallationId = requireResourceId(installationId, "installation_id");
      const expectedPromotionId = requireDigest(promotionId, "promotion_id");
      const result = await request(`/v1/installations/${installationPath(installationId)}/promotions/${promotionPath(promotionId)}`, { signal });
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
        signal: input.signal,
      });
      requireScopedIdentity(result.body, expectedInstallationId, expectedPromotionId, "apply");
      return result;
    }

    async function applyWithDrainHandshake(input) {
      const attempts = input.runtimeDrainAttempts === undefined ? 11 : input.runtimeDrainAttempts;
      const usesDefaultInterval = input.runtimeDrainIntervalMilliseconds === undefined;
      const intervalMilliseconds = input.runtimeDrainIntervalMilliseconds === undefined
        ? 2000
        : input.runtimeDrainIntervalMilliseconds;
      if (!Number.isInteger(attempts) || attempts < 1 || attempts > 180) {
        throw new Error("runtime_drain_attempts_invalid");
      }
      if (!Number.isInteger(intervalMilliseconds) || intervalMilliseconds < 100 || intervalMilliseconds > 15000) {
        throw new Error("runtime_drain_interval_invalid");
      }
      const command = Object.freeze({
        installationId: requireResourceId(input.installationId, "installation_id"),
        promotionId: requireDigest(input.promotionId, "promotion_id"),
        expectedPayloadDigest: requireDigest(input.expectedPayloadDigest, "payload_digest"),
        expectedRevision: requireGeneration(input.expectedRevision, "expected_revision", false),
        idempotencyKey: input.idempotencyKey || idempotencyKey("apply"),
        signal: input.signal,
      });
      let runtimeDrainObserved = false;
      let runtimePendingObserved = false;
      let applyAttempts = 0;
      let statusObservations = 0;
      let invalidStateConflict = null;
      for (let round = 1; round <= attempts; round += 1) {
        if (!invalidStateConflict) {
          applyAttempts += 1;
          try {
            const result = await apply(command);
            runtimePendingObserved = runtimePendingObserved || result.body.state === "runtime_pending";
            return Object.freeze({
              status: result.status,
              body: result.body,
              attempts: applyAttempts,
              runtime_drain_observed: runtimeDrainObserved,
              runtime_pending_observed: runtimePendingObserved,
              resumed_after_conflict: false,
              status_observations: statusObservations,
            });
          } catch (error) {
            if (isInvalidStateConflict(error)) {
              invalidStateConflict = error;
            } else if (!isRuntimeDrainConflict(error)) {
              throw error;
            } else {
              runtimeDrainObserved = true;
              if (round === attempts) {
                throw error;
              }
            }
          }
        }
        if (invalidStateConflict) {
          const observed = await promotion(command.installationId, command.promotionId, command.signal);
          statusObservations += 1;
          runtimePendingObserved = runtimePendingObserved || observed.body.state === "runtime_pending";
          const resolution = classifyApplyStatus(observed.body);
          if (resolution === "complete") {
            requireApplyStatusBinding(observed.body, command);
            return Object.freeze({
              status: observed.status,
              body: observed.body,
              attempts: applyAttempts,
              runtime_drain_observed: runtimeDrainObserved,
              runtime_pending_observed: runtimePendingObserved,
              resumed_after_conflict: true,
              status_observations: statusObservations,
            });
          }
          if (resolution === "terminal") {
            throw invalidStateConflict;
          }
          requireApplyStatusBinding(observed.body, command);
          if (round === attempts) {
            throw new Error("apply_resolution_timeout");
          }
        }
        if (input.signal && input.signal.aborted) {
          throw input.signal.reason || new Error(invalidStateConflict ? "apply_resume_aborted" : "runtime_drain_retry_aborted");
        }
        const delayMilliseconds = usesDefaultInterval
          ? Math.min(intervalMilliseconds * (2 ** Math.min(round - 1, 3)), 15000)
          : intervalMilliseconds;
        await sleep(delayMilliseconds);
      }
      throw new Error("runtime_drain_attempts_exhausted");
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

    async function liveRuntimeRestartConfirmation(input) {
      const installationId = requireResourceId(input.installationId, "installation_id");
      const promotionId = requireDigest(input.promotionId, "promotion_id");
      if (typeof input.processInstanceId !== "string" || !PROCESS_INSTANCE_ID.test(input.processInstanceId)) {
        throw new Error("process_instance_id_invalid");
      }
      if (typeof input.operationId !== "string" || !LIVE_RESTART_OPERATION.test(input.operationId)) {
        throw new Error("operation_id_invalid");
      }
      const shutdownBoundary = requireUtcTimestamp(input.shutdownBoundary, "shutdown_boundary");
      const product = await deployment(installationId, promotionId);
      const operational = await operationalDeployment(installationId, promotionId);
      const runtime = requireBody(operational.body.runtime, "operational_runtime");
      const attestation = requireBody(runtime.attestation, "operational_attestation");
      const serving = requireBody(runtime.serving, "operational_serving");
      const observedAt = requireUtcTimestamp(runtime.observed_at, "runtime_observed_at");
      const lastHeartbeatAt = requireUtcTimestamp(serving.last_heartbeat_at, "last_heartbeat_at");
      const leaseExpiresAt = requireUtcTimestamp(serving.lease_expires_at, "lease_expires_at");
      if (
        product.body.state !== "live" ||
        operational.body.state !== "live" ||
        runtime.phase !== "live" ||
        serving.state !== "fresh" ||
        !Number.isSafeInteger(product.body.attestation_revision) ||
        product.body.attestation_revision < 1 ||
        !Number.isSafeInteger(attestation.deployment_revision) ||
        attestation.deployment_revision < 1 ||
        attestation.process_instance_id !== input.processInstanceId ||
        product.body.attestation_revision !== attestation.deployment_revision ||
        product.body.last_serving_heartbeat !== serving.last_heartbeat_at ||
        product.body.serving_lease_expires_at !== serving.lease_expires_at ||
        compareUtcTimestamps(lastHeartbeatAt, shutdownBoundary) <= 0 ||
        compareUtcTimestamps(lastHeartbeatAt, observedAt) > 0 ||
        compareUtcTimestamps(observedAt, leaseExpiresAt) >= 0 ||
        utcDifferenceNanoseconds(leaseExpiresAt, lastHeartbeatAt) >
          SERVING_LEASE_MAXIMUM_NANOSECONDS ||
        utcDifferenceNanoseconds(observedAt, lastHeartbeatAt) >
          SERVING_LEASE_MAXIMUM_NANOSECONDS
      ) {
        throw new Error("live_restart_confirmation_invalid");
      }
      return Object.freeze({
        schema_version: 1,
        kind: LIVE_RESTART_CONFIRMATION_KIND,
        checkpoint: LIVE_FRESH_LEASE_CHECKPOINT,
        operation_id: input.operationId,
        installation_id: installationId,
        promotion_id: promotionId,
        public_origin: origin,
        shutdown_boundary: shutdownBoundary.value,
        observed_at: observedAt.value,
        product_state: product.body.state,
        operational_state: operational.body.state,
        runtime_phase: runtime.phase,
        serving_state: serving.state,
        attestation_revision: product.body.attestation_revision,
        process_instance_id: input.processInstanceId,
        last_heartbeat_at: lastHeartbeatAt.value,
        lease_expires_at: leaseExpiresAt.value,
      });
    }

  async function beginCertificationAuthoring(input) {
      const expectedGeneration = requireGeneration(
        input.expectedGeneration ?? 0,
        "expected_generation",
        true,
      );
      if (expectedGeneration !== 0) {
        throw new Error("authoring_expected_generation_invalid");
      }
      const turn = await authoringTurn({
        installationId: input.installationId,
        sessionId: input.sessionId,
        expectedGeneration,
        message: input.message,
        idempotencyKey: input.authoringIdempotencyKey,
        signal: input.signal,
      });
      const projection = projectionEvidence(turn.body);
      if (
        ![200, 201].includes(turn.status) ||
        projection.projection_state !== "preview_ready" ||
        !Number.isSafeInteger(projection.generation) ||
        projection.generation !== 1 ||
        projection.disposition !== "created" ||
        !Array.isArray(projection.model_completions) ||
        projection.model_completions.length !== 1
      ) {
        throw new Error("authoring_not_preview_ready");
      }
      const completion = projection.model_completions[0];
      const workerRequestId = requireResourceId(
        completion && completion.request_id,
        "worker_request_id",
      );
      const workerCompletionSha256 = requireDigest(
        completion && completion.completion_sha256,
        "worker_completion_sha256",
      );
      const observed = observedAt();
      const installationId = requireResourceId(input.installationId, "installation_id");
      const sessionId = requireResourceId(turn.body.session_id, "authoring_session_id");
      const generation = requireGeneration(
        turn.body.generation,
        "authoring_generation",
        false,
      );
      const candidateRulesetHash = requireDigest(
        projection.candidate_ruleset_hash,
        "candidate_ruleset_hash",
      );
      return Object.freeze({
        authoring_evidence: Object.freeze({
          schema_version: 1,
          kind: AUTHORING_EVIDENCE_KIND,
          observed_at: observed,
          public_origin: origin,
          authoring_http_status: turn.status,
          authoring_session_id: sessionId,
          authoring_generation: generation,
          expected_generation: expectedGeneration,
          authoring_disposition: projection.disposition,
          installation_id: installationId,
          one_shot: true,
          worker_request_id: workerRequestId,
          worker_completion_sha256: workerCompletionSha256,
        }),
        preview_ready_evidence: Object.freeze({
          schema_version: 1,
          kind: PREVIEW_READY_EVIDENCE_KIND,
          observed_at: observed,
          public_origin: origin,
          installation_id: installationId,
          authoring_session_id: sessionId,
          authoring_generation: generation,
          projection_state: projection.projection_state,
          candidate_ruleset_hash: candidateRulesetHash,
          worker_request_id: workerRequestId,
          worker_completion_sha256: workerCompletionSha256,
        }),
        authoring_http_status: turn.status,
        authoring: projection,
      });
    }

    async function completeCertificationDecision(input) {
      if (Object.hasOwn(input, "confirmPreview")) {
        throw new Error("preview_confirmation_override_forbidden");
      }
      if (typeof root.confirm !== "function") {
        throw new Error("native_preview_confirmation_unavailable");
      }
      const command = requireCertificationDecisionCommand(input.command);
      const installationId = command.installation_id;
      const sessionId = command.authoring_session_id;
      const generation = command.authoring_generation;
      const candidateRulesetHash = command.candidate_ruleset_hash;
      const previewCompletionChallengeSha256 =
        command.preview_completion_challenge_sha256;
      const current = await authoringSession(installationId, sessionId);
      const currentProjection = sessionProjectionEvidence(current.body);
      if (
        current.status !== 200 ||
        currentProjection.projection_state !== "preview_ready" ||
        requireGeneration(
          currentProjection.observed_generation,
          "observed_generation",
          false,
        ) !== generation ||
        currentProjection.candidate_ruleset_hash !== candidateRulesetHash
      ) {
        throw new Error("authoring_preview_identity_mismatch");
      }
      const promoted = await promote({
        installationId,
        sessionId,
        expectedGeneration: generation,
        idempotencyKey: command.promotion_idempotency_key,
      });
      if (!promoted.body || promoted.body.state !== "pending_approval") {
        throw new Error("promotion_not_pending_approval");
      }
      const preview = await approvalPreview(installationId, promoted.body.promotion_id);
      if (!preview.body || preview.body.state !== "pending_approval") {
        throw new Error("approval_preview_not_pending");
      }
      const safeSummary = summaryEvidence(preview.body.summary);
      const targetContentHash = requireDigest(
        preview.body.summary.target_content_hash,
        "target_content_hash",
      );
      if (
        promoted.body.payload_digest !== preview.body.payload_digest ||
        targetContentHash !== candidateRulesetHash
      ) {
        throw new Error("promotion_candidate_identity_mismatch");
      }
      const confirmationPrompt = Object.freeze({
        installation_id: preview.body.installation_id,
        promotion_id: preview.body.promotion_id,
        revision: preview.body.revision,
        payload_digest: preview.body.payload_digest,
        target_content_hash: targetContentHash,
        preview_completion_challenge_sha256: previewCompletionChallengeSha256,
        summary: safeSummary,
      });
      const confirmed = root.confirm(
        `Approve this Starring product preview?\n\n${JSON.stringify(confirmationPrompt, null, 2)}`,
      );
      if (confirmed !== true) {
        throw new Error("preview_not_approved_by_operator");
      }
      const chromeConfirmation = Object.freeze({
        schema_version: 1,
        kind: CHROME_PREVIEW_CONFIRMATION_KIND,
        observed_at: observedAt(),
        confirmation_surface: "chrome_confirm",
        accepted: true,
        installation_id: preview.body.installation_id,
        promotion_id: preview.body.promotion_id,
        revision: preview.body.revision,
        payload_digest: preview.body.payload_digest,
        target_content_hash: targetContentHash,
        preview_completion_challenge_sha256: previewCompletionChallengeSha256,
        summary: safeSummary,
      });
      const approved = await approve({
        installationId,
        promotionId: promoted.body.promotion_id,
        expectedPayloadDigest: preview.body.payload_digest,
        expectedRevision: preview.body.revision,
        idempotencyKey: command.approval_idempotency_key,
      });
      if (!approved.body || approved.body.state !== "approved") {
        throw new Error("promotion_not_approved");
      }
      const applied = await applyWithDrainHandshake({
        installationId,
        promotionId: promoted.body.promotion_id,
        expectedPayloadDigest: preview.body.payload_digest,
        expectedRevision: approved.body.revision,
        idempotencyKey: command.apply_idempotency_key,
        runtimeDrainAttempts: input.runtimeDrainAttempts,
        runtimeDrainIntervalMilliseconds: input.runtimeDrainIntervalMilliseconds,
        signal: input.signal,
      });
      if (!applied.body || !["runtime_pending", "live"].includes(applied.body.state)) {
        throw new Error("promotion_not_applied");
      }
      const productDecisionEvidence = Object.freeze({
        schema_version: 1,
        kind: PRODUCT_DECISION_EVIDENCE_KIND,
        observed_at: observedAt(),
        public_origin: origin,
        installation_id: requireResourceId(applied.body.installation_id, "installation_id"),
        promotion_id: requireDigest(applied.body.promotion_id, "promotion_id"),
        authoring_session_id: sessionId,
        authoring_generation: generation,
        target_content_hash: targetContentHash,
        payload_digest: requireDigest(preview.body.payload_digest, "payload_digest"),
        preview_state: preview.body.state,
        approval_state: approved.body.state,
        apply_state: applied.body.state,
        runtime_pending_observed: applied.runtime_pending_observed,
        preview_completion_challenge_sha256: previewCompletionChallengeSha256,
        chrome_confirmation: chromeConfirmation,
      });
      return Object.freeze({
        product_decision_evidence: productDecisionEvidence,
        promotion_http_status: promoted.status,
        promotion: decisionEvidence(promoted.body),
        preview_http_status: preview.status,
        preview: {
          installation_id: preview.body.installation_id,
          promotion_id: preview.body.promotion_id,
          revision: preview.body.revision,
          state: preview.body.state,
          payload_digest: preview.body.payload_digest,
          target_content_hash: targetContentHash,
          summary: safeSummary,
        },
        approval_http_status: approved.status,
        approval: decisionEvidence(approved.body),
        apply_http_status: applied.status,
        apply_attempts: applied.attempts,
        runtime_drain_observed: applied.runtime_drain_observed,
        runtime_pending_observed: applied.runtime_pending_observed,
        apply_resumed_after_conflict: applied.resumed_after_conflict,
        apply_status_observations: applied.status_observations,
        applied: {
          installation_id: applied.body.installation_id,
          promotion_id: applied.body.promotion_id,
          state: applied.body.state,
          replayed: applied.body.replayed,
        },
      });
    }

    async function runOneShotProductFlow(input) {
      if (typeof input.confirmPreview !== "function") {
        throw new Error("preview_confirmation_required");
      }
      let turn = await authoringTurn({
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
      let promoted;
      try {
        promoted = await promote({
          installationId: input.installationId,
          sessionId: input.sessionId,
          expectedGeneration: turn.body.generation,
          idempotencyKey: input.promotionIdempotencyKey,
        });
      } catch (error) {
        if (
          !error ||
          error.name !== "StarringD2ProductRequestError" ||
          error.status !== 422 ||
          error.code !== "invalid_server_candidate"
        ) {
          throw error;
        }
        const priorGeneration = turn.body.generation;
        if (priorGeneration >= Number.MAX_SAFE_INTEGER) {
          throw new Error("authoring_generation_exhausted");
        }
        const expectedFinalizedGeneration = priorGeneration + 1;
        const finalized = await authoringTurn({
          installationId: input.installationId,
          sessionId: input.sessionId,
          expectedGeneration: priorGeneration,
          message: FINALIZE_EXISTING_PREVIEW_MESSAGE,
          idempotencyKey: input.finalizationIdempotencyKey || idempotencyKey("authoring_finalize"),
          signal: input.signal,
        });
        if (
          !finalized.body ||
          !finalized.body.projection ||
          finalized.body.projection.state !== "preview_ready" ||
          finalized.body.generation !== expectedFinalizedGeneration
        ) {
          throw new Error("authoring_finalization_not_preview_ready");
        }
        turn = finalized;
        promoted = await promote({
          installationId: input.installationId,
          sessionId: input.sessionId,
          expectedGeneration: turn.body.generation,
          idempotencyKey: input.finalizedPromotionIdempotencyKey || idempotencyKey("promote_finalized"),
        });
      }
      if (!promoted.body || promoted.body.state !== "pending_approval") {
        throw new Error("promotion_not_pending_approval");
      }
      const preview = await approvalPreview(input.installationId, promoted.body.promotion_id);
      if (!preview.body || preview.body.state !== "pending_approval") {
        throw new Error("approval_preview_not_pending");
      }
      const candidateRulesetHash = requireDigest(
        projectionEvidence(turn.body).candidate_ruleset_hash,
        "candidate_ruleset_hash",
      );
      const targetContentHash = requireDigest(
        preview.body.summary.target_content_hash,
        "target_content_hash",
      );
      if (
        promoted.body.payload_digest !== preview.body.payload_digest ||
        targetContentHash !== candidateRulesetHash
      ) {
        throw new Error("promotion_candidate_identity_mismatch");
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
      const applied = await applyWithDrainHandshake({
        installationId: input.installationId,
        promotionId: promoted.body.promotion_id,
        expectedPayloadDigest: preview.body.payload_digest,
        expectedRevision: approved.body.revision,
        idempotencyKey: input.applyIdempotencyKey,
        runtimeDrainAttempts: input.runtimeDrainAttempts,
        runtimeDrainIntervalMilliseconds: input.runtimeDrainIntervalMilliseconds,
        signal: input.signal,
      });
      if (!applied.body || !["runtime_pending", "live"].includes(applied.body.state)) {
        throw new Error("promotion_not_applied");
      }
      if (![200, 201].includes(turn.status)) {
        throw new Error("authoring_http_status_invalid");
      }
      const authoringEvidence = Object.freeze({
        schema_version: 1,
        kind: AUTHORING_EVIDENCE_KIND,
        observed_at: observedAt(),
        public_origin: origin,
        authoring_http_status: turn.status,
        authoring_session_id: requireResourceId(turn.body.session_id, "authoring_session_id"),
        authoring_generation: requireGeneration(turn.body.generation, "authoring_generation", false),
        installation_id: requireResourceId(input.installationId, "installation_id"),
        one_shot: true,
      });
      const productDecisionEvidence = Object.freeze({
        schema_version: 1,
        kind: PRODUCT_DECISION_EVIDENCE_KIND,
        observed_at: observedAt(),
        public_origin: origin,
        installation_id: requireResourceId(applied.body.installation_id, "installation_id"),
        promotion_id: requireDigest(applied.body.promotion_id, "promotion_id"),
        authoring_session_id: requireResourceId(
          turn.body.session_id,
          "authoring_session_id",
        ),
        authoring_generation: requireGeneration(
          turn.body.generation,
          "authoring_generation",
          false,
        ),
        target_content_hash: targetContentHash,
        payload_digest: requireDigest(preview.body.payload_digest, "payload_digest"),
        preview_state: preview.body.state,
        approval_state: approved.body.state,
        apply_state: applied.body.state,
        runtime_pending_observed: applied.runtime_pending_observed,
      });
      return {
        authoring_evidence: authoringEvidence,
        product_decision_evidence: productDecisionEvidence,
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
        apply_attempts: applied.attempts,
        runtime_drain_observed: applied.runtime_drain_observed,
        runtime_pending_observed: applied.runtime_pending_observed,
        apply_resumed_after_conflict: applied.resumed_after_conflict,
        apply_status_observations: applied.status_observations,
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
      const pendingObservedSeed = input.pendingObserved === undefined ? false : input.pendingObserved;
      if (typeof pendingObservedSeed !== "boolean") {
        throw new Error("pending_observed_invalid");
      }
      let pendingObserved = pendingObservedSeed;
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
          operational.body.runtime.serving &&
          operational.body.runtime.serving.state === "fresh"
        ) {
          const evidenceObservedAt = observedAt();
          const exact = exactLiveProjectionEvidence(
            product,
            operational,
            evidenceObservedAt,
          );
          return Object.freeze({
            schema_version: 1,
            kind: LIVE_EVIDENCE_KIND,
            observed_at: evidenceObservedAt,
            public_origin: origin,
            pending_observed: pendingObserved,
            live_observed: true,
            attempts: attempt,
            installation_id: requireResourceId(input.installationId, "installation_id"),
            promotion_id: requireDigest(input.promotionId, "promotion_id"),
            product_state: product.body.state,
            operational_state: operational.body.state,
            runtime_phase: operational.body.runtime.phase,
            serving_state: operational.body.runtime.serving.state,
            deployment_http_status: product.status,
            operational_http_status: operational.status,
            ...exact,
          });
        }
        if (attempt < attempts) {
          await sleep(intervalMilliseconds);
        }
      }
      throw new Error("deployment_live_timeout");
    }

    async function waitForLiveLoss(input) {
      const attempts = input.attempts === undefined ? 60 : input.attempts;
      const intervalMilliseconds = input.intervalMilliseconds === undefined ? 2000 : input.intervalMilliseconds;
      const installationId = requireResourceId(input.installationId, "installation_id");
      const promotionId = requireDigest(input.promotionId, "promotion_id");
      if (!Number.isInteger(attempts) || attempts < 1 || attempts > 180) {
        throw new Error("attempts_invalid");
      }
      if (!Number.isInteger(intervalMilliseconds) || intervalMilliseconds < 100 || intervalMilliseconds > 10000) {
        throw new Error("poll_interval_invalid");
      }
      for (let attempt = 1; attempt <= attempts; attempt += 1) {
        let product;
        let operational;
        let publicProblem = null;
        try {
          product = await deployment(installationId, promotionId);
        } catch (error) {
          if (
            !error ||
            error.name !== "StarringD2ProductRequestError" ||
            error.status !== 503 ||
            error.retryable !== true ||
            typeof error.code !== "string" ||
            !RESOURCE_ID.test(error.code)
          ) {
            throw error;
          }
          publicProblem = error;
        }
        try {
          operational = await operationalDeployment(installationId, promotionId);
        } catch (error) {
          if (
            !error ||
            error.name !== "StarringD2ProductRequestError" ||
            error.status !== 503 ||
            error.retryable !== true ||
            typeof error.code !== "string" ||
            !RESOURCE_ID.test(error.code)
          ) {
            throw error;
          }
          publicProblem = publicProblem || error;
        }
        const runtime = operational && operational.body.runtime
          ? requireBody(operational.body.runtime, "operational_runtime")
          : null;
        const serving = runtime && runtime.serving
          ? requireBody(runtime.serving, "operational_serving")
          : null;
        const stillLive = Boolean(
          product &&
          operational &&
          product.body.state === "live" &&
          operational.body.state === "live" &&
          runtime &&
          runtime.phase === "live" &&
          serving &&
          serving.state === "fresh"
        );
        const gatewayDisconnected = Boolean(
          product &&
          operational &&
          product.status === 200 &&
          operational.status === 200 &&
          product.body.state === "pending" &&
          operational.body.state === "pending" &&
          runtime &&
          runtime.phase === "live" &&
          serving &&
          serving.state === "disconnected"
        );
        if (!stillLive) {
          return Object.freeze({
            schema_version: 1,
            kind: LIVE_LOSS_EVIDENCE_KIND,
            observed_at: observedAt(),
            public_origin: origin,
            installation_id: installationId,
            promotion_id: promotionId,
            live_lost: true,
            deployment_http_status: product ? product.status : publicProblem.status,
            operational_http_status: operational ? operational.status : publicProblem.status,
            product_state: product && product.body.state ? product.body.state : "unavailable",
            operational_state: operational && operational.body.state ? operational.body.state : "unavailable",
            runtime_phase: runtime && runtime.phase ? runtime.phase : "unavailable",
            serving_state: serving && serving.state ? serving.state : "unavailable",
            public_code: gatewayDisconnected
              ? "runtime_gateway_disconnected"
              : publicProblem
                ? publicProblem.code
                : "live_state_lost",
            retryable: gatewayDisconnected
              ? true
              : publicProblem
                ? publicProblem.retryable
                : false,
          });
        }
        if (attempt < attempts) {
          await sleep(intervalMilliseconds);
        }
      }
      throw new Error("deployment_live_loss_timeout");
    }

    async function runReplacementFlow(input) {
      const sourcePromotionId = requireDigest(input.sourcePromotionId, "source_promotion_id");
      if (!new Set(["update", "rollback"]).has(input.replacementKind)) {
        throw new Error("replacement_kind_invalid");
      }
      const result = await runOneShotProductFlow(input);
      const live = await waitForLive({
        installationId: input.installationId,
        promotionId: result.promotion.promotion_id,
        attempts: input.liveAttempts,
        intervalMilliseconds: input.liveIntervalMilliseconds,
        pendingObserved: result.product_decision_evidence.runtime_pending_observed,
      });
      if (result.promotion.promotion_id === sourcePromotionId) {
        throw new Error("replacement_promotion_not_distinct");
      }
      return Object.freeze({
        schema_version: 1,
        kind: REPLACEMENT_EVIDENCE_KIND,
        observed_at: observedAt(),
        public_origin: origin,
        installation_id: result.promotion.installation_id,
        source_promotion_id: sourcePromotionId,
        replacement_promotion_id: result.promotion.promotion_id,
        replacement_kind: input.replacementKind,
        preview_state: result.preview.state,
        approval_state: result.approval.state,
        apply_state: result.applied.state,
        pending_observed: live.pending_observed,
        live_observed: live.live_observed,
        product_state: live.product_state,
        operational_state: live.operational_state,
        runtime_phase: live.runtime_phase,
        serving_state: live.serving_state,
        drain_conflict_observed: result.runtime_drain_observed,
        drain_attempts: result.apply_attempts,
      });
    }

    return Object.freeze({
      me,
      authorityCheck,
      authenticationEvidence,
      discordInteractionObservation,
      authoringTurn,
      authoringSession,
      promote,
      promotion,
      approvalPreview,
      approve,
      apply,
      applyWithDrainHandshake,
      deployment,
      operationalDeployment,
      liveRuntimeRestartConfirmation,
      beginCertificationAuthoring,
      completeCertificationDecision,
      createCertificationDecisionCommand,
      runOneShotProductFlow,
      waitForLive,
      waitForLiveEvidence: waitForLive,
      waitForLiveLoss,
      runReplacementFlow,
    });
  }

  Object.defineProperty(root, "StarringD2ProductDriver", {
    value: Object.freeze({ create: createD2ProductDriver }),
    configurable: false,
    enumerable: true,
    writable: false,
  });
})(globalThis);
