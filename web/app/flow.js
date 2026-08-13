const AUTHORING_CONTEXT_KEY = "starring.authoring.context.v1";
const RESOURCE_ID = /^[A-Za-z0-9_.:-]{1,128}$/;

export function loadAuthoringContext(storage, freshSessionId) {
  const fallback = { installationId: "", sessionId: freshSessionId, generation: 0 };
  try {
    const raw = storage.getItem(AUTHORING_CONTEXT_KEY);
    if (!raw) return fallback;
    const value = JSON.parse(raw);
    if (Object.keys(value).sort().join(",") !== "generation,installation_id,schema_version,session_id"
      || value.schema_version !== 1
      || (value.installation_id !== "" && !RESOURCE_ID.test(value.installation_id))
      || !RESOURCE_ID.test(value.session_id)
      || !Number.isSafeInteger(value.generation)
      || value.generation < 0) {
      return fallback;
    }
    return {
      installationId: value.installation_id,
      sessionId: value.session_id,
      generation: value.generation,
    };
  } catch {
    return fallback;
  }
}

export function persistAuthoringContext(storage, state) {
  try {
    storage.setItem(AUTHORING_CONTEXT_KEY, JSON.stringify({
      schema_version: 1,
      installation_id: state.installationId,
      session_id: state.sessionId,
      generation: state.generation,
    }));
  } catch {
    return false;
  }
  return true;
}

export function invalidateApprovalFlow(state) {
  state.flowEpoch = (state.flowEpoch || 0) + 1;
  state.promotion = null;
  state.approvalPreview = null;
  state.approved = null;
  return state.flowEpoch;
}

export function captureFlowToken(state) {
  return Object.freeze({
    flowEpoch: state.flowEpoch || 0,
    installationId: state.installationId,
    sessionId: state.sessionId,
    generation: state.generation,
  });
}

export function isFlowTokenCurrent(state, token) {
  return Boolean(token)
    && token.flowEpoch === (state.flowEpoch || 0)
    && token.installationId === state.installationId
    && token.sessionId === state.sessionId
    && token.generation === state.generation;
}
