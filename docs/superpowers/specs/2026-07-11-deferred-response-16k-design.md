# Phase 16k — Deferred Response Lifecycle 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프 + Claude live)
- **범위**: Phase 16k — interaction 응답 수명주기를 "처리 중 → 완료/실패"로 정확히 닫기. `DeferEphemeral` + `EditResponse` 액션 + edit-original + runner-level 실패 fallback. core + runtime + tool 풀스택.
- **선행**: 16a~16j. 16j RespondEphemeral-first의 정식 버전.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. no_ai_gateway 유지. **16k는 풀스택**(core: 액션/seam/run·handle_event / runtime: twilight responder / tool: 시나리오) — core/runtime 둘 다 수정.

**목표(한 문장):** 긴 mutation 흐름을 `DeferEphemeral`로 3초 ACK하고, 성공 시 원본 응답을 완료 메시지로 edit, 실패 시 runner가 원본을 실패 메시지로 edit.

---

## 0. 범위

**포함:** `ActionSpec::DeferEphemeral` + `ActionSpec::EditResponse{content}` · `InteractionResponder::defer_ephemeral()` + `edit_response(content)` (default-unsupported) · run(Defer/Edit arm) · **handle_event 실패 fallback**(defer된 rule이 실패하면 edit-original로 실패 통지, failure_message 앱 제공) · validate(Defer-first/one-initial/edit-after-defer) · Mock 테스트 · **twilight 실구현**(defer=DeferredChannelMessageWithSource, edit=update_response) · StudyRoom 16k 시나리오 · live smoke(Claude).

**제외(→후속):** FollowupEphemeral · 여러 followup/progress update · retry/backoff · 앱별 실패 메시지 커스터마이즈(고정 문자열) · 실패 원인 상세 노출.

---

## 1. 응답 수명주기

Discord: interaction은 initial response 1개(3초 내). 이후 `update_response`(원본 편집) 또는 `create_followup`(새 메시지). 16k는:
```
ModalSubmit → DeferEphemeral(초기 응답 = "처리 중")
           → mutation들
           → 성공: EditResponse(원본 → "완료")   [rule action]
           → 실패: runner/handle_event가 원본 → "실패"  [fallback]
```
- **성공 완료 = rule의 EditResponse.** 실패 = handle_event fallback(rule은 정상 경로만; DSL을 workflow engine으로 만들지 않음).
- **ButtonClick → OpenModal 은 defer 금지**(modal이 초기 응답). Defer는 긴 ModalSubmit 흐름 전용.

---

## 2. 액션 + seam

### automation-state
```rust
enum ActionSpec {
    ..., PostPanel {..},
    DeferEphemeral,                       // unit variant → {"type":"defer_ephemeral"}
    EditResponse { content: String },     // always-template
}
```

### automation-core
```rust
enum PlannedAction {
    ..., DeferEphemeral, EditResponse { content: String },
}

// InteractionResponder trait (default-unsupported 추가)
async fn defer_ephemeral(&self) -> Result<(), AdapterError>;
async fn edit_response(&self, content: String) -> Result<(), AdapterError>;
```
Mock: `ResponderCall::{DeferEphemeral, EditResponse{content}}` 기록.

---

## 3. run / handle_event

- **run**(순수): `DeferEphemeral` → `responder.defer_ephemeral()`. `EditResponse{content}` → render(content, EphemeralMessageContent) → `responder.edit_response(rendered)`. 나머지 불변. run은 exhaustive 위해 두 arm 보유(직접 테스트용).
- **handle_event**(defer ACK 성공 추적 + 실패 fallback + failure_message):
```rust
pub async fn handle_event(
    event, ruleset, bindings, mutation, responder,
    failure_message: &str,
) -> Result<HandleOutcome, AdapterError> {
    match interpret(...) {
        Some(plan) => {
            let context = RuntimeContext::from_event(event);
            let mut steps = plan.steps;
            let defer_acked = if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
                responder.defer_ephemeral().await?;   // Defer 실패 시 return Err (원본 없음 → fallback 안 함)
                steps.remove(0);                       // ACK 성공 → 벗겨내고 나머지 실행
                true
            } else {
                false
            };
            match run(&context, &ActionPlan { steps }, mutation, responder).await {
                Ok(_) => Ok(HandleOutcome::Executed),
                Err(error) => {
                    if defer_acked {
                        if let Ok(rendered) =
                            render(failure_message, &context, SanitizeContext::EphemeralMessageContent)
                        {
                            let _ = responder.edit_response(rendered).await;   // best-effort
                        }
                    }
                    Err(error)
                }
            }
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
```
**보강①**: fallback은 **defer ACK가 실제 성공했을 때만**(Defer 자체 실패면 `?`로 return, edit 시도 안 함). handle_event가 Defer를 직접 실행·추적하고 나머지 plan을 run에 넘김(strip). **보강(failure render)**: failure_message는 **render+sanitize**(always-template, `${input.x}` 가능) — 렌더 실패(오설정)면 edit 생략(로그만). core에 문구 하드코딩 없음. failure_message는 **앱 제공**(tool→gateway::run→runner→handle_event). **handle_event 시그니처 변경 → 호출자(runner + 테스트) 갱신**(컴파일러 가이드).

---

## 4. validate (Defer/Edit 계약 — 6종)

rule의 action을 훑어 defer/edit index 수집 후:
1. **DeferNotFirst{rule}** — DeferEphemeral이 index 0 아님(2번째 defer도 여기서 걸림). 3초 ACK + handle_event `first()` 일치.
2. **ConflictingInitialResponse{rule}** — DeferEphemeral + (RespondEphemeral|OpenModal)(초기 응답 2개 금지).
3. **EditResponseWithoutDefer{rule}** — EditResponse 있는데 DeferEphemeral 없음.
4. **DeferredMissingEditResponse{rule}** — DeferEphemeral 있는데 EditResponse 없음(성공 완료 통지 누락 → "처리 중" 매달림).
5. **MultipleEditResponse{rule}** — EditResponse >1개.
6. **EditResponseNotLast{rule}** — EditResponse가 마지막 action 아님(완료 후 뒤 action 실행되는 이상 UX).

+ EditResponse.content 템플릿 검사(check_template — ModalSubmit 컨텍스트 input).

**Defer rule 계약**: `[DeferEphemeral(0), ...work..., EditResponse(마지막, 정확히 1개)]`. 새 ValidationError 6종. (일반 "RespondEphemeral 2개" 기존 갭은 범위 밖; progress/multiple-edit는 후속.)

---

## 5. twilight responder (automation-runtime)
- **defer_ephemeral**: `send(InteractionResponse{ kind: DeferredChannelMessageWithSource, data: Some(InteractionResponseData{ flags: Some(EPHEMERAL), ..default }) })`.
- **edit_response**: `http.interaction(app_id).update_response(&token).content(Some(&content)).await` (원본 편집). twilight 0.17 `update_response` API는 플랜에서 대조.

---

## 6. StudyRoom 16k 시나리오 (tool)
submit 룰(16j RespondEphemeral-first → defer/edit):
```
- { defer_ephemeral }
- { create_role, ... } / { create_channel, ... } / { upsert_overwrite ×2 } / { grant_role } / { post_panel, ... }
- { edit_response, content: "스터디룸 '${input.room_name}' 생성 완료! 새 채널을 확인하세요." }
```
gateway::run에 failure_message 전달(예: "스터디룸 생성에 실패했습니다. 봇 권한 또는 역할 순서를 확인해주세요."). study_help 룰 유지. 시작 시 validate.

---

## 7. Task 구성 (한 Phase, core/runtime/tool)
- **Task A (core)**: automation-state 액션 2 + automation-core(plan/interpret/run arm + handle_event failure_message + validate 3 + Mock responder + lib) + Mock 테스트(defer/edit call 순서, 실패 fallback edit, validate 3, EditResponse 템플릿).
- **Task B (runtime)**: automation-runtime responder(defer_ephemeral/edit_response) + gateway/runner에 failure_message 배선.
- **Task C (tool + live)**: tool StudyRoom defer/edit 시나리오 + failure_message. **Claude live smoke**(재사용 토큰: 처리 중→완료 확인, 실패 케이스도 관찰 가능하면).

---

## 8. 제약 (validate + 문서)
- Defer는 초기 응답 1개 원칙 — 같은 rule에 Respond/OpenModal와 공존 불가, index 0.
- EditResponse는 Defer 이후만.
- ButtonClick→OpenModal은 Defer 금지(modal이 초기 응답).
- 실패 메시지 = 앱 제공 failure_message를 **render+sanitize**(always-template) 후 edit-original. 렌더 실패(오설정) → edit 생략(로그만). 사용자엔 안전 안내, 로그엔 상세 error(Err 전파). 상세 내부 error는 사용자에 노출 안 함.
- **Defer rule 계약**: DeferEphemeral(index 0, 1개) + EditResponse(마지막, 정확히 1개), Respond/OpenModal 공존 불가. ButtonClick→OpenModal은 Defer 금지.

---

## 9. 하지 않는 것 (Forbidden — 후속)
FollowupEphemeral · progress update · retry/backoff · 앱별 실패 메시지 커스터마이즈 · 실패 원인 상세 노출 · action-level 실패 분기(DSL workflow engine 금지).

---

## 10. 로드맵
```
16j✅ StudyRoom live   16k▶ Deferred response lifecycle (이 스펙 — defer/edit-original + 실패 fallback)
후속 트랙: Dynamic instance buttons → Public join registry → join live / DB 영속 / Backend API / Web UI
```

---

## 11. Codex 핸드오프 (개요)
1. automation-state: ActionSpec::DeferEphemeral(unit) + EditResponse{content} + serde 테스트.
2. automation-core: PlannedAction 2 + InteractionResponder(defer_ephemeral/edit_response default-unsupported) + run arm 2 + handle_event(failure_message + **defer ACK 성공 추적/strip** + failure render fallback) + validate 6(DeferNotFirst/ConflictingInitialResponse/EditResponseWithoutDefer/DeferredMissingEditResponse/MultipleEditResponse/EditResponseNotLast) + Mock(ResponderCall 2) + lib. **handle_event 호출자(runner+테스트) 갱신.**
3. automation-runtime: responder(defer_ephemeral=DeferredChannelMessageWithSource, edit_response=update_response) + gateway::run/runner에 failure_message.
4. tool: StudyRoom defer/edit 시나리오 + failure_message. 주석 없음. 게이트 build/test/clippy/fmt.
5. **Claude live**(재사용 토큰).

## 최종 정리
16k = deferred response lifecycle. DeferEphemeral(3초 ACK "처리 중") → mutation → 성공 시 EditResponse가 원본을 "완료"로 edit, 실패 시 handle_event가 원본을 실패 메시지로 edit(runner-level fallback, failure_message 앱 제공). run은 순수(arm만), handle_event가 defer-aware 실패 통지. validate가 Defer-first/one-initial/edit-after-defer 강제. twilight edit=update_response. StudyRoom UX를 "처리 중→완료/실패"로 완성.
