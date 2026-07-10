# Phase 16c — Modal / OpenModal Core 설계 스펙

- **작성일**: 2026-07-10
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16c — 모달 왕복(버튼→모달→제출→정적 액션)을 순수 코어로. **입력값은 캡처·보존만, interpolation 없음.**
- **선행**: Phase 16a(rule core), 16b(live edge, live 검증 완료). 아키텍처: `2026-07-10-interaction-rule-plane-design.md`

---

## ⚠️ 최상위 원칙 (불변)
AI는 설치 시점 설계자, Runtime은 저장된 rule의 결정론적 interpreter, 이벤트-타임 LLM 호출 금지. **16c는 순수 코어(Mock)** — live는 16d.

---

## 0. 목적 · 경계

모달 **왕복 루프 자체**를 순수 코어에서 증명한다:
```
ButtonClick → OpenModal → ModalSubmit → input capture → static actions
```
`RespondEphemeral("${input.name}")` 같은 에코를 넣는 순간 16c는 모달이 아니라 **템플릿 엔진 Phase**가 된다 → 금지. 입력값은 **RuntimeEvent에 캡처·보존만** 하고, 실제 소비/템플릿/sanitize/dynamic materialization은 **16e**.

**핵심 문장:** *16c는 modal input을 RuntimeEvent에 캡처하고 보존한다. 하지만 action content나 resource name에 interpolation하지 않는다.*

---

## 1. 확정 결정

| # | 결정 |
|---|---|
| D1 | 입력값 **캡처만** — static action 유지, interpolation 없음 |
| D2 | `InteractionResponder::open_modal` 추가하되 **default = Unsupported** (16b 안 깨짐) |
| D3 | 16c는 순수 코어 — automation-state/automation-core만 수정, **automation-runtime 무수정**(default open_modal 상속) |
| D4 | OpenModal은 interaction initial response 계열 → 기존 responder seam 그대로(16d에서 실구현) |
| D5 | 2-rule 체인: ButtonClick→OpenModal / ModalSubmit→actions, **modal key로 연결** |

---

## 2. 타입 모델

### automation-state (스키마 확장, 전부 `#[serde(deny_unknown_fields)]`)
```
InteractionRuleSet { version, panels, modals: Vec<ModalSpec>, rules }   // modals 필드 추가(serde default)

struct ModalSpec { key: String, title: String, fields: Vec<ModalFieldSpec> }
struct ModalFieldSpec { key: String, label: String, style: ModalFieldStyle, #[serde(default)] required: bool }
enum ModalFieldStyle { Short, Paragraph }                                // snake_case

enum TriggerSpec { ButtonClick { component }, ModalSubmit { modal } }    // ModalSubmit 추가
enum ActionSpec  { GrantRole {..}, RespondEphemeral {..}, OpenModal { modal: String } }  // OpenModal 추가
```

### automation-core (런타임 확장)
```
enum EventKind {
    ButtonClick { component: String },
    ModalSubmit { modal: String, inputs: BTreeMap<String, String> },     // 추가 — 입력값 캡처
}

struct ModalPresentation { modal: String, title: String, fields: Vec<ModalFieldSpec> }   // 해소된 모달(자체 소유)
enum PlannedAction {
    GrantRole { role: RoleId, target: UserId },
    RespondEphemeral { content: String },
    OpenModal(ModalPresentation),                                        // 추가
}

enum AdapterErrorKind { Forbidden, NotFound, RateLimited, Network, Unsupported, Unknown }  // Unsupported 추가

trait InteractionResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError>;
    async fn open_modal(&self, modal: &ModalPresentation) -> Result<(), AdapterError> {    // default = Unsupported
        Err(AdapterError::new(AdapterErrorKind::Unsupported, "open_modal is not supported"))
    }
}
```
> `ModalPresentation`은 modal **key**(logical) + title + fields만 — Discord custom_id 인코딩은 edge(16d) 책임(16b custom_id 경계와 동일). 입력값(inputs)은 **RuntimeEvent에만** 있고 16c의 static plan으로 흘리지 않는다(16e에서 소비).

---

## 3. validate 책임 (automation-core)
16a 검증 + 추가:
- **modal key 유일성** (ruleset.modals).
- **modal field key 유일성** (각 모달 내부).
- `OpenModal { modal }`이 존재하는 modal key 참조.
- `ModalSubmit { modal }` trigger가 존재하는 modal key 참조.
- (기존) 미지원 trigger/action/필드는 closed enum + deny_unknown_fields로 거부.

---

## 4. interpret 책임 (automation-core, LLM 없음)
- **ButtonClick 이벤트** → ButtonClick trigger 매칭 → actions 처리. `OpenModal` action이면 ruleset.modals에서 modal key 해소 → `PlannedAction::OpenModal(ModalPresentation{...})`. modal 미해소 시 None(validate가 선차단).
- **ModalSubmit 이벤트** → `ModalSubmit { modal }` trigger를 event.modal_key와 매칭 → 정적 actions로 ActionPlan. **event.inputs는 매칭에 안 쓰이고 소비도 안 됨(캡처·보존만).**
- 매칭 없음 → None(no-op).

---

## 5. run / Mock
- `run`: `PlannedAction::OpenModal(m)` → `responder.open_modal(&m).await?`. 나머지 16a 그대로.
- `MockInteractionResponder`: `open_modal` override → `ResponderCall::OpenModal { modal }` 기록.
- default(미override) responder의 open_modal은 `Unsupported` 반환 → run이 Err로 실패 기록(테스트 11).

---

## 6. 테스트 (12)
1. `ModalSpec` serde roundtrip (automation-state).
2. 중복 modal key → validate fail.
3. 중복 modal field key(모달 내부) → validate fail.
4. OpenModal이 없는 modal key 참조 → validate fail.
5. ModalSubmit trigger가 없는 modal key 참조 → validate fail.
6. ButtonClick(create_study_button) → `OpenModal` ActionPlan(title/fields 해소 확인).
7. Mock responder가 `open_modal` 호출 기록.
8. **ModalSubmit(study_modal) + inputs 캡처됨** — 이벤트가 modal_key/field values 보존, interpret가 정적 plan 생성(입력값 안 잃음).
9. ModalSubmit trigger 매칭 → 정적 `RespondEphemeral` ActionPlan.
10. unknown modal submit → NoOp.
11. default(unsupported) responder로 OpenModal 실행 → run이 `Unsupported` 오류로 실패.
12. conditions/template 등 unknown 필드 여전히 deny(신규 타입 deny_unknown_fields).

MVP 대표 rule:
```
modals: [ { key: study_room_modal, title: "스터디룸 생성", fields: [ { key: room_name, label: "방 이름", style: short } ] } ]
rules:
  - { key: open_study_modal,   trigger: {button_click, create_study_button}, actions: [ {open_modal, study_room_modal} ] }
  - { key: submit_study_modal, trigger: {modal_submit, study_room_modal},    actions: [ {respond_ephemeral, "요청이 접수되었습니다."} ] }
```
room_name은 RuntimeEvent에 캡처되지만 content에 안 들어간다(16c 경계).

---

## 7. 하지 않는 것 (Forbidden — 16e/후속)
`${input.xxx}` 템플릿 · 에코 응답 · sanitize · length limit · dynamic field/role/channel key · CreateChannel/CreateRole action · **live Discord modal callback(16d)** · DB · event-time AI.

---

## 8. 스코프 경계 · 무수정
- ✅ automation-state(ModalSpec/ModalFieldSpec/OpenModal/ModalSubmit), automation-core(EventKind::ModalSubmit/ModalPresentation/PlannedAction::OpenModal/open_modal default/Mock/validate/interpret/run).
- ❌ **automation-runtime 무수정**(default open_modal 상속으로 16b 컴파일 유지). live/DB/template/dynamic 없음.

---

## 9. 로드맵 위치
```
16a ✅ Rule core   16b ✅ Button live   16c ▶ Modal core (이 스펙)
16d   Modal live edge (TwilightInteractionResponder.open_modal 실구현 + modal custom_id 인코딩 + ModalSubmit 수신)
16e   Template/sanitize/dynamic action core (${input}, bounded create)
16f   StudyRoom north-star
```

---

## 10. Codex 핸드오프 (개요)
1. automation-state: modal.rs(ModalSpec/ModalFieldSpec/ModalFieldStyle) + rule.rs(ModalSubmit/OpenModal + InteractionRuleSet.modals). 전부 deny_unknown_fields. serde 테스트.
2. automation-core: event.rs(ModalSubmit), plan.rs(ModalPresentation/OpenModal), adapter.rs(Unsupported + open_modal default), mock.rs(open_modal override + ResponderCall::OpenModal), validate.rs(modal 규칙 4종), interpret.rs(두 이벤트 kind), run.rs(OpenModal 실행).
3. **automation-runtime 절대 무수정** — default open_modal로 컴파일 유지 확인.
4. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. 완료 후 push. **live/토큰 없음.**
5. 기존 189 테스트 무변경 + 신규 ~12 → ~201.

## 최종 정리
16c = 모달 왕복 순수 코어. 입력값 캡처·보존(RuntimeEvent) + 정적 액션. interpolation/sanitize/dynamic은 16e. open_modal은 트레잇에 default-unsupported로 추가 → 16b 안 깨지고 16d에서 실구현. 이 경계가 Starring답게 "모달의 성공/실패"와 "템플릿의 성공/실패"를 분리한다.
