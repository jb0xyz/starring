const STATE_LABELS = Object.freeze({
  needs_input: "추가 정보 필요",
  discussion: "설계 중",
  capability_gap: "지원 범위 확인",
  unsupported: "현재 미지원",
  rejected: "안전 정책으로 거절",
  preview_ready: "미리보기 준비됨",
});

const SUMMARY_LABELS = Object.freeze({
  panels: "패널",
  modals: "입력창",
  rules: "동작 규칙",
  actions: "실행 단계",
  target_version: "목표 버전",
  required_approvals: "필요 승인",
});

export function projectionModel(projection) {
  if (!projection || projection.schema_version !== 1 || typeof projection.state !== "string") {
    throw new TypeError("unsupported_projection");
  }
  const draft = projection.preview?.draft || projection.draft || {};
  const summary = Object.entries(SUMMARY_LABELS)
    .filter(([key]) => Number.isSafeInteger(draft[key]) && draft[key] >= 0)
    .map(([key, label]) => ({ key, label, value: draft[key] }));
  const diagnostics = [];
  if (Array.isArray(projection.capabilities)) diagnostics.push(...projection.capabilities.map(String));
  if (Array.isArray(draft.unresolved_references)) {
    diagnostics.push(...draft.unresolved_references.map((value) => `연결 필요: ${String(value)}`));
  }
  return {
    state: projection.state,
    stateLabel: STATE_LABELS[projection.state] || "알 수 없는 상태",
    tone: projection.state === "preview_ready" ? "good" : ["unsupported", "rejected", "capability_gap"].includes(projection.state) ? "warn" : "neutral",
    assistantMessage: typeof projection.assistant_message === "string" ? projection.assistant_message : "응답을 표시할 수 없습니다.",
    summary,
    diagnostics,
    previewReady: projection.state === "preview_ready" && Boolean(projection.preview),
  };
}

export function approvalSummaryModel(summary) {
  if (!summary || typeof summary !== "object") return [];
  return Object.entries(SUMMARY_LABELS)
    .filter(([key]) => Number.isSafeInteger(summary[key]) && summary[key] >= 0)
    .map(([key, label]) => ({ key, label, value: summary[key] }));
}

export function safeErrorMessage(error) {
  const known = {
    session_required: "Discord 로그인이 필요합니다.",
    forbidden: "이 서버에서 필요한 권한을 확인할 수 없습니다.",
    not_found: "설치 또는 자동화를 찾을 수 없습니다.",
    conflict: "다른 변경이 먼저 반영되었습니다. 최신 상태를 다시 확인해 주세요.",
    authoring_saturated: "현재 설계 요청이 많습니다. 잠시 후 다시 시도해 주세요.",
    dependency_timeout: "외부 서비스 응답이 늦습니다. 잠시 후 다시 시도해 주세요.",
  };
  return known[error?.code] || "요청을 완료하지 못했습니다. 상태를 확인한 뒤 다시 시도해 주세요.";
}
