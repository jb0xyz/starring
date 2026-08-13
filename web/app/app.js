import { StarringApi } from "/app/api.js";
import {
  captureFlowToken,
  invalidateApprovalFlow,
  isFlowTokenCurrent,
  loadAuthoringContext,
  persistAuthoringContext,
} from "/app/flow.js";
import { approvalSummaryModel, projectionModel, safeErrorMessage } from "/app/projection.js";

const api = new StarringApi();
const savedContext = loadAuthoringContext(localStorage, crypto.randomUUID());
const state = {
  ...savedContext,
  flowEpoch: 0,
  promotion: null,
  approvalPreview: null,
  approved: null,
};
persistAuthoringContext(localStorage, state);

const byId = (id) => document.getElementById(id);
const elements = Object.fromEntries([
  "principal-name", "login-link", "logout-button", "connection-status", "installation-form",
  "installation-id", "conversation-feed", "authoring-form", "authoring-message", "send-button",
  "new-session-button", "generation-label", "projection-badge", "empty-review", "review-content",
  "assistant-message", "summary-section", "summary-grid", "diagnostics-section", "diagnostics-list",
  "deployment-section", "deployment-timeline", "promote-button", "approve-button", "apply-button", "toast",
].map((id) => [id, byId(id)]));

function setHidden(element, hidden) { element.classList.toggle("hidden", hidden); }
function setBusy(button, busy) { button.disabled = busy; button.setAttribute("aria-busy", String(busy)); }
function setConnection(text, tone = "neutral") { elements["connection-status"].textContent = text; elements["connection-status"].dataset.tone = tone; }

function setContextLocked(locked) {
  elements["installation-id"].disabled = locked;
  elements["installation-form"].querySelector("button[type=submit]").disabled = locked;
  elements["authoring-message"].disabled = locked;
  elements["send-button"].disabled = locked;
  elements["new-session-button"].disabled = locked;
}

function toast(message) {
  elements.toast.textContent = message;
  setHidden(elements.toast, false);
  window.setTimeout(() => setHidden(elements.toast, true), 4200);
}

function addMessage(kind, text) {
  const article = document.createElement("article");
  article.className = `message message-${kind}`;
  const paragraph = document.createElement("p");
  paragraph.textContent = text;
  article.append(paragraph);
  elements["conversation-feed"].append(article);
  elements["conversation-feed"].scrollTop = elements["conversation-feed"].scrollHeight;
}

function replaceSummary(items) {
  elements["summary-grid"].replaceChildren();
  for (const item of items) {
    const wrapper = document.createElement("div");
    const term = document.createElement("dt");
    const detail = document.createElement("dd");
    term.textContent = item.label;
    detail.textContent = String(item.value);
    wrapper.append(term, detail);
    elements["summary-grid"].append(wrapper);
  }
  setHidden(elements["summary-section"], items.length === 0);
}

function replaceDiagnostics(items) {
  elements["diagnostics-list"].replaceChildren();
  for (const item of items) {
    const listItem = document.createElement("li");
    listItem.textContent = item;
    elements["diagnostics-list"].append(listItem);
  }
  setHidden(elements["diagnostics-section"], items.length === 0);
}

function showProjection(projection) {
  const model = projectionModel(projection);
  setHidden(elements["empty-review"], true);
  setHidden(elements["review-content"], false);
  elements["projection-badge"].textContent = model.stateLabel;
  elements["projection-badge"].dataset.tone = model.tone;
  elements["assistant-message"].textContent = model.assistantMessage;
  replaceSummary(model.summary);
  replaceDiagnostics(model.diagnostics);
  setHidden(elements["promote-button"], !model.previewReady);
  setHidden(elements["approve-button"], true);
  setHidden(elements["apply-button"], true);
  addMessage("assistant", model.assistantMessage);
}

function resetApprovalFlow() {
  invalidateApprovalFlow(state);
  setHidden(elements["promote-button"], true);
  setHidden(elements["approve-button"], true);
  setHidden(elements["apply-button"], true);
  setHidden(elements["deployment-section"], true);
  for (const item of elements["deployment-timeline"].children) item.dataset.state = "pending";
}

function startFreshConversation(message) {
  resetApprovalFlow();
  state.sessionId = crypto.randomUUID();
  state.generation = 0;
  updateGeneration(0);
  elements["conversation-feed"].replaceChildren();
  addMessage("assistant", message);
  setHidden(elements["empty-review"], false);
  setHidden(elements["review-content"], true);
  elements["projection-badge"].textContent = "대기 중";
  delete elements["projection-badge"].dataset.tone;
}

function updateGeneration(generation) {
  if (Number.isSafeInteger(generation)) state.generation = generation;
  persistAuthoringContext(localStorage, state);
  elements["generation-label"].textContent = state.generation > 0 ? `${state.generation}번째 설계` : "새 대화";
}

function timeline(stage) {
  const stages = ["preview", "approval", "apply", "live"];
  const current = stages.indexOf(stage);
  for (const item of elements["deployment-timeline"].children) {
    const index = stages.indexOf(item.dataset.stage);
    item.dataset.state = index < current ? "done" : index === current ? "active" : "pending";
  }
  setHidden(elements["deployment-section"], false);
}

function requireInstallation() {
  const value = elements["installation-id"].value.trim();
  if (!value) throw new Error("installation_required");
  return value;
}

async function loadPrincipal() {
  try {
    const principal = await api.me();
    elements["principal-name"].textContent = principal.display_name;
    setHidden(elements["logout-button"], false);
    setHidden(elements["login-link"], true);
  } catch (error) {
    elements["principal-name"].textContent = "로그인 필요";
    setHidden(elements["logout-button"], true);
    setHidden(elements["login-link"], false);
    if (error?.status !== 401) toast(safeErrorMessage(error));
  }
}

elements["installation-id"].value = state.installationId;
updateGeneration(state.generation);
loadPrincipal();

elements["installation-form"].addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const installationId = requireInstallation();
    setConnection("권한 확인 중");
    const descriptor = await api.descriptor(installationId);
    const installationChanged = state.installationId !== installationId;
    state.installationId = installationId;
    if (installationChanged) {
      startFreshConversation("새 서버에 연결했습니다. 원하는 동작을 설명해 주세요.");
    } else {
      persistAuthoringContext(localStorage, state);
    }
    setConnection(`연결됨 · 기능 ${descriptor.actions.length}개`, "good");
  } catch (error) {
    setConnection("연결 확인 실패", "warn");
    toast(safeErrorMessage(error));
  }
});

elements["authoring-form"].addEventListener("submit", async (event) => {
  event.preventDefault();
  const message = elements["authoring-message"].value.trim();
  if (!message) return;
  try {
    const installationId = requireInstallation();
    if (installationId !== state.installationId) throw new Error("installation_not_verified");
    resetApprovalFlow();
    const token = captureFlowToken(state);
    setBusy(elements["send-button"], true);
    addMessage("user", message);
    elements["authoring-message"].value = "";
    const response = await api.authoringTurn(
      installationId,
      state.sessionId,
      state.generation,
      message,
      crypto.randomUUID(),
    );
    if (!isFlowTokenCurrent(state, token)) return;
    updateGeneration(response.generation ?? state.generation);
    showProjection(response.projection);
  } catch (error) {
    const localMessage = error.message === "installation_required"
      ? "먼저 설치 ID를 입력해 주세요."
      : error.message === "installation_not_verified"
        ? "설치 ID를 바꾼 뒤에는 연결 확인을 먼저 해 주세요."
        : null;
    toast(localMessage || safeErrorMessage(error));
  } finally {
    setBusy(elements["send-button"], false);
  }
});

elements["new-session-button"].addEventListener("click", () => {
  startFreshConversation("새 대화를 시작했습니다. 원하는 동작을 설명해 주세요.");
});

elements["promote-button"].addEventListener("click", async () => {
  try {
    const installationId = requireInstallation();
    const token = captureFlowToken(state);
    setBusy(elements["promote-button"], true);
    setContextLocked(true);
    const promotion = await api.promote(installationId, state.sessionId, state.generation);
    if (!isFlowTokenCurrent(state, token)) return;
    const approvalPreview = await api.approvalPreview(installationId, promotion.promotion_id);
    if (!isFlowTokenCurrent(state, token)) return;
    state.promotion = promotion;
    state.approvalPreview = approvalPreview;
    replaceSummary(approvalSummaryModel(state.approvalPreview.summary));
    timeline("approval");
    setHidden(elements["promote-button"], true);
    setHidden(elements["approve-button"], false);
  } catch (error) {
    toast(safeErrorMessage(error));
  } finally {
    setContextLocked(false);
    setBusy(elements["promote-button"], false);
  }
});

elements["approve-button"].addEventListener("click", async () => {
  try {
    const installationId = requireInstallation();
    const token = captureFlowToken(state);
    setBusy(elements["approve-button"], true);
    setContextLocked(true);
    const approved = await api.approve(
      installationId,
      state.promotion.promotion_id,
      state.approvalPreview.payload_digest,
      state.approvalPreview.revision,
    );
    if (!isFlowTokenCurrent(state, token)) return;
    state.approved = approved;
    timeline("apply");
    setHidden(elements["approve-button"], true);
    setHidden(elements["apply-button"], false);
  } catch (error) {
    toast(safeErrorMessage(error));
  } finally {
    setContextLocked(false);
    setBusy(elements["approve-button"], false);
  }
});

elements["apply-button"].addEventListener("click", async () => {
  try {
    const installationId = requireInstallation();
    const token = captureFlowToken(state);
    setBusy(elements["apply-button"], true);
    setContextLocked(true);
    await api.apply(
      installationId,
      state.promotion.promotion_id,
      state.approvalPreview.payload_digest,
      state.approved.revision,
    );
    if (!isFlowTokenCurrent(state, token)) return;
    timeline("apply");
    for (let attempt = 0; attempt < 40; attempt += 1) {
      const deployment = await api.deployment(installationId, state.promotion.promotion_id);
      if (!isFlowTokenCurrent(state, token)) return;
      if (deployment.state === "live") {
        timeline("live");
        elements["projection-badge"].textContent = "라이브";
        elements["projection-badge"].dataset.tone = "good";
        setHidden(elements["apply-button"], true);
        toast("자동화가 라이브 상태가 되었습니다.");
        return;
      }
      if (deployment.state === "failed") throw new Error("deployment_failed");
      await new Promise((resolve) => window.setTimeout(resolve, 1500));
    }
    throw new Error("deployment_pending");
  } catch (error) {
    toast(error.message === "deployment_pending" ? "배포가 계속 진행 중입니다. 잠시 후 상태를 다시 확인해 주세요." : safeErrorMessage(error));
  } finally {
    setContextLocked(false);
    setBusy(elements["apply-button"], false);
  }
});

elements["logout-button"].addEventListener("click", async () => {
  try { await api.logout(); } catch { }
  window.location.assign("/app");
});
