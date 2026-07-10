# Phase 16d — Modal Live Edge (Gateway) 설계 스펙

- **작성일**: 2026-07-10
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16d — 실제 버튼 클릭 → 실제 모달 팝업 → 제출 → ModalSubmit Gateway 수신 → automation-core 해석 → 정적 ephemeral
- **선행**: 16a(rule core), 16b(button live 검증완료), 16c(modal core). 16b의 live edge(`automation-runtime`) 확장.

---

## ⚠️ 최상위 원칙 (불변)
AI는 설치 시점 설계자, Runtime은 저장된 rule의 결정론적 interpreter, 이벤트-타임 LLM 호출 금지. **automation-core/automation-state 무수정** — 16d는 edge(automation-runtime + tool)만.

---

## 0. 목적

16b가 버튼을 live로 증명했듯, **모달을 live로**. 목표 시나리오:
```
버튼 클릭 → 실제 모달 팝업 → 폼 제출 → MODAL_SUBMIT 수신
→ automation-runtime custom_id decode + field 추출 → RuntimeEvent::ModalSubmit
→ automation-core interpret → 정적 RespondEphemeral
```
16e(템플릿/동적 생성) 전에 **실세계 interaction 제약**(OpenModal callback, modal custom_id roundtrip, MODAL_SUBMIT 수신, field 값 추출, 3초 응답)을 먼저 부딪힌다. 이건 코어 테스트로 안 되고 edge에서 확인해야 한다.

---

## 1. 범위: Gateway 얇은 live smoke

16b와 동일하게 **Gateway** 방식(로컬 봇 토큰 즉시 연결). 응답은 `/interactions/{id}/{token}/callback` HTTP, initial 3초, token follow-up 15분.

**포함:** TwilightInteractionResponder::open_modal 실구현 · modal custom_id encode/decode · Gateway MODAL_SUBMIT 수신 · ModalSubmit payload → RuntimeEvent::ModalSubmit · text input 값 추출 · 정적 RespondEphemeral · interaction-smoke live.

**제외:** `${input.xxx}` 템플릿 · 입력값 echo · sanitize · CreateChannel/CreateRole · 스터디룸 생성 · DB · HTTP interaction endpoint · event-time AI.

---

## 2. 확정 결정

| # | 결정 |
|---|---|
| D1 | Gateway 방식 MODAL_SUBMIT 수신 (16b와 동일) |
| D2 | automation-core/automation-state **무수정** — edge만 |
| D3 | responder는 interaction별 생성(id/token/app_id bind), 16b 패턴 유지 |
| D4 | **custom_id에 type 세그먼트 추가** — `starring:<guild>:<ruleset>:<type>:<key>` (16b 4→5 세그먼트 리팩터, 버튼도 통일) |
| D5 | OpenModal은 ButtonClick interaction 응답으로만(MODAL callback은 MODAL_SUBMIT/PING엔 금지) |
| D6 | 정적 응답 유지 — 입력값 안 씀 ("요청이 접수되었습니다." 류) |

---

## 3. custom_id 5-세그먼트 리팩터 (D4 — 16b codec 변경)

16b: `starring:<guild>:<ruleset>:<button_key>` (4). 16d: **type 판별자 추가**로 button/modal/(향후 select) 혼재 시 파싱 안정:
```
Button:  starring:<guild_id>:<ruleset_key>:button:<button_key>
Modal:   starring:<guild_id>:<ruleset_key>:modal:<modal_key>
```
automation-runtime `custom_id.rs` 변경:
- `encode_button(guild, ruleset, button_key) -> String`
- `encode_modal(guild, ruleset, modal_key) -> String`
- `decode(&str) -> Result<ParsedCustomId, CustomIdError>` — `ParsedCustomId { guild_id, ruleset_key, kind: ComponentKind, key: String }`, `enum ComponentKind { Button, Modal }`. prefix/세그먼트 수/type 검증. 5-세그먼트 아니면 거부.

**automation-core는 여전히 custom_id 포맷을 모른다** — button_key/modal_key만 처리. 16b의 custom_id 테스트는 5-세그먼트로 갱신, interaction-smoke 버튼 패널도 `encode_button` 사용.

---

## 4. ruleset_key 배선 (설계 귀결)

modal custom_id는 **responder가 생성**하는데 responder는 guild(interaction)·modal_key(ModalPresentation)는 알아도 **ruleset_key를 모른다**. 그래서:
- `gateway::run(token, ruleset_key: String, ruleset, bindings)` — ruleset_key를 파라미터로 받음.
- `TwilightInteractionResponder`에 `guild_id`(interaction에서) + `ruleset_key`(gateway에서 주입) 추가 → open_modal이 `encode_modal(guild_id, ruleset_key, modal.key)`로 모달 custom_id 구성.
- interaction-smoke 버튼 패널도 같은 ruleset_key로 `encode_button`.

(16b 버튼은 tool이 custom_id를 만들어 RULESET_KEY 상수를 이미 가졌지만, 모달은 responder가 만들어 이 배선이 필요.) ruleset_key는 네임스페이싱이고, smoke는 단일 ruleset이라 상수.

---

## 5. open_modal 실구현 (responder.rs)

`TwilightInteractionResponder::open_modal(&self, modal: &ModalPresentation)`:
- Discord modal 응답 = `InteractionResponseType::Modal`(=9). data = `InteractionResponseData { custom_id: Some(encode_modal(...)), title: Some(modal.title), components: Some(vec![각 field를 TextInput ActionRow로]), ..default }`.
- 각 `ModalFieldSpec` → `Component::TextInput(TextInput { custom_id: field.key, label: Some(field.label), style: Short|Paragraph, required: Some(field.required), .. })`, ActionRow로 감쌈.
- `http.interaction(app_id).create_response(interaction_id, token, &response)`.

MODAL callback은 ButtonClick interaction 응답으로만 유효(D5) — automation-core interpret가 ButtonClick→OpenModal plan을 만들 때만 호출됨.

---

## 6. MODAL_SUBMIT 수신 + field 추출 (convert.rs · gateway.rs)

- gateway 루프: `next_event`에 `INTERACTION_CREATE`. `Event::InteractionCreate` → interaction.data 분기:
  - `MessageComponent` → 버튼(16b 경로): custom_id decode(kind=Button) → RuntimeEvent::ButtonClick.
  - `ModalSubmit` → **신규**: modal custom_id decode(kind=Modal) → modal_key; components에서 각 TextInput의 custom_id(field key)+value 추출 → `inputs: BTreeMap` → RuntimeEvent::ModalSubmit { modal_key, inputs, guild_id, actor }.
- `convert::interaction_to_event`가 두 분기 모두 처리(또는 kind별 헬퍼). MessageComponent/ModalSubmit 아니면 무시(no-op).

---

## 7. smoke 시나리오 (11단계 — tool)

1. 채널에 "스터디룸 만들기" 버튼 패널 설치(encode_button).
2. 사용자 버튼 클릭 → Gateway MESSAGE_COMPONENT 수신.
3. custom_id decode → RuntimeEvent::ButtonClick.
4. automation-core interpret → OpenModal(create_study_modal) plan.
5. responder.open_modal → **실제 Discord 모달 팝업**.
6. 사용자 room_name 입력·제출 → Gateway MODAL_SUBMIT 수신.
7. modal custom_id decode + field 추출 → RuntimeEvent::ModalSubmit { create_study_modal, {room_name: "..."} }.
8. automation-core interpret → 정적 RespondEphemeral.
9. Discord ephemeral 표시.
10. 로그에 HandleOutcome / interaction id.
11. (입력값은 아직 안 씀 — 정적 문구.)

demo ruleset: modals[create_study_modal{room_name:short}] + rules[ButtonClick(create_study_button)→OpenModal(create_study_modal), ModalSubmit(create_study_modal)→RespondEphemeral("요청이 접수되었습니다.")].

---

## 8. 완료 기준

- 버튼 클릭 시 모달이 뜬다.
- 모달 제출 시 Gateway MODAL_SUBMIT 수신.
- modal custom_id가 modal_key로 복원.
- text input 값이 RuntimeEvent에 캡처.
- **automation-core 무수정 작동.**
- 정적 ephemeral 표시.
- `cargo test`는 토큰 없이 green(순수 = custom_id 5-세그먼트 테스트 + no_ai_gateway).
- live smoke는 env로만 수동.

**Claude 검증(토큰 없이):** 컴파일 + custom_id 순수 테스트 + clippy + fmt. 실제 클릭·제출은 사용자 수동(또는 Claude가 셋업 후 사용자 클릭, 16b처럼).

---

## 9. 스코프 경계
- ✅ automation-runtime(custom_id 리팩터, convert ModalSubmit 분기, open_modal 실구현, gateway ruleset_key), tools/interaction-smoke(모달 시나리오).
- ❌ **automation-core/automation-state 무수정.** template/sanitize/dynamic/CreateChannel/CreateRole/DB/AI 없음.

---

## 10. 로드맵
```
16a✅ 16b✅ 16c✅ 16d▶ Modal live   16e Template/sanitize/dynamic   16f StudyRoom
```

---

## 11. Codex 핸드오프 (개요)
1. automation-runtime: custom_id.rs(5-세그먼트, ComponentKind, encode_button/encode_modal/decode) + convert.rs(ModalSubmit 분기 + field 추출) + responder.rs(open_modal 실구현, TextInput 빌드, guild_id/ruleset_key 보유) + gateway.rs(ruleset_key 파라미터). 16b custom_id 테스트 5-세그먼트로 갱신.
2. tools/interaction-smoke: 모달 포함 demo ruleset + encode_button 패널 + gateway::run(ruleset_key, ...).
3. **automation-core/automation-state 절대 무수정.**
4. twilight 0.17 실제 API(InteractionResponseType::Modal, TextInput/TextInputStyle, ModalInteractionData/components 추출)는 플랜 단계 소스 대조.
5. 주석 없음. 게이트 build/test(토큰 없이 green)/clippy/fmt. 완료 후 push. **실제 클릭·제출은 사용자 수동.**

## 최종 정리
16d = Gateway 기반 Modal live smoke. 버튼→실제 모달→제출→MODAL_SUBMIT→automation-core(무수정)→정적 ephemeral. custom_id 5-세그먼트(type)로 리팩터, ruleset_key를 responder로 배선. 이게 되면 16e(입력값 실사용)를 실세계 제약 안 채로 안전하게 설계할 수 있다.
