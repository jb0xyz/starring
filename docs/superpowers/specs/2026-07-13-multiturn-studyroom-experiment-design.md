# Multi-turn StudyRoom Experiment — Handoff Spec

**작성:** Claude (설계) · **수행:** 홈서버 AI (eval 인프라 + gemma 접근 보유) · **날짜:** 2026-07-13

## 목적

Stage-2 최대 미지수의 마지막 조각을 값싸게 확정한다: **무료·로컬 gemma 12b로 복잡 설계(StudyRoom)를 완성할 수 있는가.**

지금까지 결과: gemma 12b는 **원샷 11액션**(`studyroom_full`)에서 완성률 0%. 그러나 **4-5개 상호의존 도구는 일관되게 자율 조립**(7회 실험 확정). 방향 결정(사용자 확정 Ⓐ) = **모델을 바꾸지 말고 과제를 모델 크기에 맞게 분해**. 이 실험이 그 가설을 검증한다.

**가설:** StudyRoom을 gemma가 잘하는 크기(턴당 2-7 도구)의 5턴으로 쪼개면, 누적 Draft가 `studyroom_full`과 구조적으로 동일한 완성 상태(1 패널·1 모달·2 규칙·11 액션·golden trace simulate)에 도달한다.

**성공 판정:** 누적 Draft가 `studyroom_full` 기대치(아래 표)를 만족하고 최종 턴에서 실제 validate + golden-trace simulate 통과 → **무료·로컬로 복잡 설계 viable 증명**. 실패해도 어느 턴에서 무너지는지가 다음 레버를 알려준다.

## 이 실험이 성립하는 근거 (검증된 사실)

`crates/design-harness/src/tools.rs`의 모든 액션 추가 도구는 `rule_key: String` 필드를 받아 `append_action(draft, &rule_key, action)`로 규칙에 붙는다(예: `add_grant_role_action`, `add_resource_action`, `add_upsert_overwrite_action`, `add_post_panel_action`, `set_register_instance`, `add_interaction_action`의 `OpenModal/DeferEphemeral/EditResponse/...`). 액션은 "직전에 begin_rule한 규칙"이 아니라 **key로 지정한 규칙**에 붙는다. Draft는 세션 전체에 걸쳐 누적된다. → **하나의 규칙(`submit_room`)을 여러 턴에 걸쳐 이어짓는 것이 가능**하다(턴 3에서 `begin_rule submit_room` + 일부 액션, 턴 4에서 `rule_key="submit_room"`으로 나머지 액션 append). 이 실험은 **cases.yaml 추가만으로** 가능하며 Rust/assertions 변경이 필요 없다.

## 전역 제약 (반드시 준수)

- **주석 금지** (`//`, `///`, `//!` 전부) — Starring 컨벤션.
- **엔진·안전경계 크레이트 무변경.** 이 작업 범위 = `eval/design-harness/` (Part A) + `crates/design-harness/`·`tools/design-harness/` 내부 모듈 재구성 (Part B).
- **lib 순수성 유지:** `crates/design-harness`(lib)는 sqlx/twilight/rusqlite/sqlite/sled/redb 금지(`dependency_guard` 테스트가 강제). SQLite는 `tools/design-harness`(CLI 엣지)에만.
- **게이트 그린 유지:** `cargo test --workspace`(현재 653 pass/0 fail) · `cargo clippy --workspace -- -D warnings` 0 · `cargo fmt --check` 0. Promptfoo JS 테스트도 그린.
- **비밀 금지:** Bearer API key·게이트웨이 base URL은 실행 머신 env/Keychain에서만. 코드/문서/커밋에 절대 넣지 말 것.
- **base 브랜치:** `feat/harness-serving-improvements` (현재 HEAD 4b6bc93). Part A와 Part B는 **각각 별도 커밋**.

---

## Part A — `studyroom_incremental` eval 케이스

### A.1 최종 누적 Draft 목표 구조 (`studyroom_full`과 동일해야 함)

| 항목 | 값 |
|---|---|
| top-level 패널 | 1 (`study_panel`, 버튼 `create_study_room` static) |
| 모달 | 1 (`study_modal`, 필드 `room_name` required short) |
| 규칙 | 2 (`open_modal`, `submit_room`) |
| 액션 총합 | 11 (open_modal 1 + submit_room 10) |
| distinct mutation 도구 | ≥10 |
| 최종 게이트 | 실제 validate + StudyRoom golden trace simulate |

**규칙별 액션 배치:**
- `open_modal` (트리거: `create_study_room` 버튼 클릭): `[OpenModal(study_modal)]` — 1 액션
- `submit_room` (트리거: `study_modal` 제출): `[DeferEphemeral, CreateRole(member_role), CreateChannel(room_channel), UpsertOverwrite(everyone view 거부), UpsertOverwrite(member_role view 허용), GrantRole(member_role→actor), PostPanel(welcome_panel: Help static + Close instance), PostPanel(hub_panel: Join instance), RegisterInstance(study_instance, manifest=member_role/room_channel/welcome_panel/hub_panel), EditResponse("Created ${input.room_name}")]` — 10 액션

`welcome_panel`/`hub_panel`은 `add_post_panel_action`으로 **런타임 게시**되는 인스턴스 패널이므로 top-level 패널 수(1)에 포함되지 않는다(`studyroom_full`과 동일 계산).

### A.2 5턴 분해 (턴당 도구 부하)

| 턴 | 하위 목표 | 예상 도구 | 도구 수 | validate |
|---|---|---|---|---|
| 1 | 표면: `study_panel`+`create_study_room` 버튼, `study_modal`+`room_name` | add_panel, add_button, add_modal | 3 | 안 함 |
| 2 | `open_modal` 규칙 | begin_rule, add_interaction_action(OpenModal) | 2 | 안 함 |
| 3 | `submit_room` 규칙 파트1(리소스 생성) | begin_rule, add_interaction_action(Defer), add_resource_action×2, add_upsert_overwrite_action×2, add_grant_role_action | 7 | 안 함 |
| 4 | `submit_room` 규칙 파트2(패널 게시·등록·응답) | add_post_panel_action×2, set_register_instance, add_interaction_action(EditResponse) | 4 | 안 함 |
| 5 | 전체 validate + golden-trace simulate | validate_draft, simulate_draft | 2 | **함** |

가장 무거운 턴(3)도 7 도구로, gemma 실증 범위(4-5)를 약간 넘는 수준. 턴 3이 반복해서 걸리면 두 턴(리소스 생성 → 권한 오버라이트)으로 더 쪼갠다(값싼 후속 조치).

전 대화 누적 distinct mutation 도구 = add_panel, add_button, add_modal, begin_rule, add_interaction_action, add_resource_action, add_upsert_overwrite_action, add_grant_role_action, add_post_panel_action, set_register_instance = **10종** (minDistinctMutationTools:10 충족).

### A.3 `eval/design-harness/cases.yaml`에 추가할 케이스 (복사용 초안)

기존 파일 끝에 아래 케이스를 append 한다. 턴 발화는 `studyroom_full`과 동일한 key·값을 쓰되 5턴으로 분할했다.

```yaml
- description: StudyRoom incremental multi-turn build
  vars:
    caseId: studyroom_incremental
    request: |-
      {
        "schema_version": 1,
        "turns": [
          {
            "id": "surface",
            "input": "Discord StudyRoom 자동화를 여러 턴에 걸쳐 조립할 거야. 이번 턴에는 표면 구조만 만들어라. study_hub 채널에 key=study_panel, content=Create a study room 패널을 선언하고 label=Create room, static key=create_study_room 버튼을 넣어라. 그리고 key=study_modal, title=Create study room 모달에 required short 필드 key=room_name, label=Room name을 둬라. 규칙이나 action은 아직 만들지 말고 validate나 simulate도 하지 마라. 이번 턴 변경만 마치고 다음 지시를 기다려라."
          },
          {
            "id": "open-rule",
            "input": "기존 study_panel과 study_modal은 그대로 보존해라. 이번 턴에는 key=open_modal 규칙만 추가한다. 이 규칙은 create_study_room 버튼 클릭으로 시작하고 study_modal을 여는 action 하나만 가진다. 아직 validate나 simulate 하지 말고 이번 턴 변경만 마쳐라."
          },
          {
            "id": "submit-resources",
            "input": "기존 설계는 모두 보존해라. 이번 턴에는 key=submit_room 규칙을 시작한다. 이 규칙은 study_modal 제출로 시작하고 순서대로 다음 action을 추가한다: ephemeral defer, key=member_role 역할(${input.room_name} members) 생성, key=room_channel 채널(study-${input.room_name}) 생성, everyone의 view_channel 거부, member_role의 view_channel 허용, actor에게 member_role 부여. 여기까지만 하고 이번 턴을 마쳐라. 아직 패널 게시나 인스턴스 등록이나 validate나 simulate는 하지 마라."
          },
          {
            "id": "submit-finalize",
            "input": "submit_room 규칙에 이어서 나머지 action을 추가한다. 기존 action은 중복 추가하지 말고 그 뒤에 붙여라: 생성된 채널에 key=welcome_panel, content=Welcome to ${input.room_name} 패널을 게시하되 static Help=study_help 버튼과 instance action Close=close 버튼을 포함하고, study_hub에 key=hub_panel, content=${input.room_name} is open 패널을 게시하되 instance action Join=join 버튼을 포함하고, key=study_instance kind=study_room 인스턴스를 등록하되 manifest에 member_role, room_channel, welcome_panel, hub_panel을 같은 alias로 모두 포함하고, 마지막으로 Created ${input.room_name}로 응답을 수정한다. 아직 validate나 simulate는 하지 말고 이번 턴 변경만 마쳐라."
          },
          {
            "id": "validate-simulate",
            "input": "이제 설계가 끝났다. 더 이상 Draft를 바꾸지 말고 현재 revision을 validate하고 StudyRoom golden trace로 simulate해서 미리보기 가능한 준비 상태로 끝내라. 질문하지 마라."
          }
        ]
      }
    expectedOutcomes: "ready,completed"
    inputTurnCount: 5
    minChangedTurns: 4
    requireSimulation: true
    requireActualValidation: true
    requireActualSimulation: true
    expectedPanels: 1
    expectedModals: 1
    expectedRules: 2
    expectedActions: 11
    minDistinctMutationTools: 10
    maxModelCalls: 48
    maxToolCalls: 96
    maxModelCallsPerTurn: 12
    maxToolCallsPerTurn: 24
  assert:
    - type: javascript
      value: file://assertions.js:terminalOutcome
    - type: javascript
      value: file://assertions.js:conversationFlow
    - type: javascript
      value: file://assertions.js:actualGateStamps
    - type: javascript
      value: file://assertions.js:finalGates
    - type: javascript
      value: file://assertions.js:draftShape
    - type: javascript
      value: file://assertions.js:taskSemantics
    - type: javascript
      value: file://assertions.js:distinctMutationTools
    - type: javascript
      value: file://assertions.js:noExcessiveRepeatedErrors
    - type: javascript
      value: file://assertions.js:callBudgets
    - type: javascript
      value: file://assertions.js:perTurnBudgets
```

### A.4 코드와 반드시 대조할 두 결합점 (홈서버 AI가 판단)

당신이 소유한 `session.rs` 로직에 맞춰 위 발화를 조정할 것. 나(Claude)는 의도만 지정한다.

1. **StudyRoom simulation 트리거 문구:** 시스템 프롬프트에 "The harness deterministically enables StudyRoom simulation from the exact human message; set validate to true whenever the human explicitly says StudyRoom"이라 되어 있다. 트리거가 **정확한 문구/토큰 매칭**이면, 턴 5의 발화("StudyRoom golden trace로 simulate")가 그 트리거를 확실히 켜도록 맞추고, **턴 1-4는 그 트리거를 켜지 않도록**(validate=false로 남도록) 문구를 유지하라. `studyroom_full`의 종결 문구와 턴 5를 동일하게 두면 안전하다.
2. **턴당 강제 validation 여부:** 하네스가 매 턴 종료 시 validation을 강제한다면, 턴 1(패널+모달, 규칙 0)은 불완전 Draft로 validation 실패→halt 위험. `set_turn_brief`의 `validate` 플래그가 턴 1-4에서 false로 남는지 확인하라. 강제된다면 발화에 "이번 턴은 검증하지 말라"를 더 강하게 명시하거나, 하네스가 불완전 Draft 턴을 `progressed`로 정상 종료하는지 확인하라(기존 `additive_revision`이 중간 턴을 어떻게 종료하는지 참고).

### A.5 실행 전 확인

- 케이스 추가 후 `additive_revision`(기존 2턴 다중턴 케이스)이 여전히 통과하는지 — 회귀 없음 확인.
- 누적 Draft 구조가 `studyroom_full`과 동일한지: `draftShape`(패널1/모달1/규칙2/액션11)·`distinctMutationTools`(≥10)·`finalGates`(validate+simulate) 단언이 최종 상태에 걸린다.
- 턴 1-4는 `progressed`, 턴 5는 `ready`로 끝나는 흐름이 `conversationFlow`·`terminalOutcome` 단언과 맞는지.

### A.6 측정 리포트 (measurements.md에 새 섹션 추가)

`eval/design-harness/measurements.md`에 기존 항목과 동일한 정직 기준(warmup 제외·3run·회귀 인정·과장 금지)으로 추가:

- **표(케이스 = studyroom_incremental):** 기존 컬럼 + **턴별** 관측을 함께 — 각 턴의 model calls / tool calls / distinct mutation tools / 어느 턴에서 stall/halt 했는지. (perTurnBudgets가 이미 턴별 카운트를 추적하므로 telemetry에서 뽑을 수 있음 — assertions.js 확장 불필요.)
- **완성 판정:** 최종 pass rate / completion rate / validation rate / required-simulation rate.
- **원샷 대비:** 같은 조건의 `studyroom_full`(원샷 0%)과 나란히 두어, 분해가 완성률을 실제로 올렸는지 정직하게 비교.
- **stall 턴 분석:** 완성 실패 시 어느 턴·어느 도구에서 무너졌는지(예: 턴 3의 7 도구 부하 초과 → 6턴 분해 필요 신호).

---

## Part B — 유지보수: 큰 파일 모듈 분리

서빙·adaptive 작업으로 코드가 소수 파일에 몰렸다(사용자 지적). **동작 보존** 리팩터로 응집도를 회복하되, 실험 안정성을 위해 **Part A 측정 이후 별도 커밋**으로 한다.

### B.1 현재 큰 파일

| 파일 | 라인 | 성격 |
|---|---|---|
| `crates/design-harness/src/session.rs` | 2659 | 에이전트 루프 + RepairState 머신 + routed_tools + append_anchor/fit_context + snapshot/restore + phase 머신 + 도구 dispatch — **책임 다수** |
| `crates/design-harness/src/tools.rs` | 1484 | 도구 DTO(패널/모달/규칙/액션/인스턴스) + dispatch |
| `crates/design-harness/src/turn.rs` | 1242 | 턴 brief/scope/finish 파싱 + 스코프 요구 |

### B.2 분리 방침 (책임 단위, 기술 계층 아님)

한 파일은 한 책임. 예시 경계(홈서버 AI가 실제 응집 구조에 맞게 판단 — 아래는 강제가 아니라 방향):

- **session.rs → 분리 후보:** ① 복구 상태머신(`RepairState`/`RepairTicket`/전이) → `repair.rs` · ② 도구 라우팅(`routed_tools`/phase별 노출) → `routing.rs` · ③ 컨텍스트 관리(`append_anchor`/`fit_context`/anchor 렌더링) → `context.rs` · ④ 스냅샷/복원 → `snapshot.rs` · 핵심 run 루프와 dispatch만 `session.rs`에 잔류.
- **tools.rs → 분리 후보:** DTO 정의(도메인별: `tools/dto_*.rs` 또는 묶음)와 dispatch/append 로직 분리. 단일 파일이 너무 크면 도메인별(panel/modal/rule/action/instance) 그룹화.
- **turn.rs:** 상대적으로 응집됨(턴 프로토콜) — 급하지 않으면 유지 가능. 명확히 분리 가능한 조각만.

### B.3 제약

- **동작 보존:** public API·직렬화·도구 이름·오류 코드 무변경. **순수 모듈 이동/분할**이지 로직 재작성 아님.
- **게이트 그린:** 653 pass 유지 · clippy 0 · fmt 0 · 주석 0 · `dependency_guard`(lib 순수성) 유지.
- **테스트 배치:** 테스트도 대응 모듈로 옮기되 커버리지 손실 0.
- **Part A와 별도 커밋** — eval 실험과 리팩터를 섞지 말 것(리뷰·회귀 추적 위해).
- 과잉 분해 금지 — 100줄짜리를 10개로 쪼개지 말 것. **응집된 책임 단위**로만.

---

## 순서·산출물

1. **Part A 먼저** (additive·저위험): cases.yaml 추가 → 3run 측정 → measurements.md 섹션 + 정직한 완성 판정. **이게 Ⓐ 가설의 실제 답.** (별도 커밋)
2. **Part B 다음** (동작 보존 리팩터): 큰 3파일 모듈 분리, 별도 커밋, 게이트 그린.
3. **종결 — main에 PR + merge** (사용자 지시): Part A·B 완료 + 로컬 게이트 그린 후, `feat/harness-serving-improvements`를 push하고 **main 대상 PR을 연다**. PR에는 이 브랜치 전체(서빙 개선 + adaptive 대화형 설계 + studyroom_incremental eval + 유지보수 리팩터)가 담긴다.
   - PR 본문: 무엇을 바꿨는지(서빙/adaptive/eval/refactor) · 측정 요약(원샷 0% 대비 incremental 완성률) · 게이트 결과 · 알려진 tradeoff(복잡 원샷 지연 회귀는 접은 경로).
   - **GitHub Actions CI(checks + postgres 잡)가 그린이어야 함** — PR이 자동 트리거.
   - `gh`가 인증돼 있으면 홈서버 AI가 `gh pr create`로 열고, 아니면 "PR 준비 완료"로 보고 → 사용자 Mac의 Codex가 연다.
   - **self-merge 금지.** merge는 **Claude 독립 검증(게이트 재현·스코프 diff·측정 재현) 통과 후**에 한다 — 우리 안전 계약(보고 불신·검증 후 착지) 유지. 최종 상태 = main에 merged.

**보고 형식(각 Part별):** 변경 파일 스코프 · 게이트 결과(테스트 수/clippy/fmt) · Part A는 측정 표(턴별 포함)+원샷 대비+완성 판정, Part B는 분리 전후 파일 구조 · 종결 시 **PR 링크 + CI 상태**. **Claude가 독립 검증**하므로(게이트 재현·스코프 diff·측정 재현) 보고는 사실만.

## 검증 계약 (Claude)

- Part A: cases.yaml diff가 데이터 추가만인지 · 누적 Draft가 `studyroom_full` 구조와 동일 목표인지 · 측정이 정직한지(원샷 대비·stall 턴) · 완성 주장이 **studyroom_incremental**(복잡)에 대한 것인지(단순 케이스와 혼동 금지).
- Part B: 스코프가 design-harness 내부 모듈 이동만인지 · 동작 보존(public API/직렬화/도구명/오류코드 무변경) · 게이트 그린 재현 · lib 순수성.
- **merge 게이트:** PR을 self-merge 하지 않았는지 · 브랜치 전체 스코프(엔진/안전경계 무변경) · CI 그린 · 위 Part A·B 검증 통과. **전부 통과해야 merge.** 최종 = main에 merged.
