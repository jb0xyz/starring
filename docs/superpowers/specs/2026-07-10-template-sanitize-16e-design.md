# Phase 16e — Template + Sanitize Core (RespondEphemeral) 설계 스펙

- **작성일**: 2026-07-10
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16e — ModalSubmit에서 캡처한 input 값을 안전하게 ephemeral 응답에 사용. **template 엔진 + context-aware sanitize를 RespondEphemeral content에만.** 순수 코어.
- **선행**: 16c(modal core: EventKind::ModalSubmit{inputs} 캡처), 16d(modal live). automation-runtime 무수정.

---

## ⚠️ 최상위 원칙 (불변)
AI는 설치시점 설계자, Runtime은 결정론적 interpreter, 이벤트-타임 LLM 금지. **16e는 순수 코어(Mock)** — automation-core만 수정, **automation-runtime 무수정.**

**경계:** 입력값을 **응답 메시지에만** 쓴다. 채널/역할/권한/패널을 입력값으로 **생성하지 않는다**(16f).

---

## 0. 목적

```
ModalSubmit inputs("room_name":"코딩방") → RespondEphemeral "방 이름: ${input.room_name}"
→ 치환 → sanitize → "방 이름: 코딩방" ephemeral
```
template/입력값 사용 = **보안 경계**. 가장 안전한 표면(제출자 본인만 보는 ephemeral)에서 template 엔진 + sanitize를 먼저 완성한다. 동적 생성(서버 상태 변경)은 16f.

---

## 1. 확정 결정 (D1~D8)

- **D1.** `RespondEphemeral.content`는 `String` 유지.
- **D2.** 모든 content 문자열은 template로 해석.
- **D3.** placeholder 없는 문자열은 literal로 렌더.
- **D4.** 16e 지원 placeholder는 `${input.<field_key>}` 만.
- **D5.** missing input / 미지원 placeholder는 **에러**, 절대 빈 문자열 대체 금지.
- **D6.** 렌더 결과는 항상 `EphemeralMessageContent` sanitizer 통과.
- **D7.** RespondEphemeral 스키마 변경 없음.
- **D8.** 동적 resource name은 스코프 밖.

---

## 2. content 문자열 처리 규칙 (박아둘 것)

1. 모든 `RespondEphemeral.content`는 template로 파싱.
2. placeholder 없으면 literal로 렌더.
3. 지원 placeholder는 `${input.<field_key>}` 만.
4. missing input은 에러. 빈 문자열 대체 금지.
5. 잘못된 placeholder 문법은 에러.
6. 렌더 결과는 항상 EphemeralMessageContent sanitize 통과.
7. sanitize 실패/길이 초과는 action failure로 기록.

**escape:** *Literal `${` output escaping is not supported in Phase 16e. Any `${...}` sequence is interpreted as a template expression.* (모호하게 두지 않고 "미지원"으로 명시. `$$`/`\$` escape는 후속.)

---

## 3. 타입 (automation-core `template.rs` 신설)

```rust
pub enum SanitizeContext { EphemeralMessageContent }   // 16f: ChannelName, RoleName 추가

pub enum TemplateError {
    BadSyntax(String),
    UnsupportedVariable(String),
    MissingInput(String),
    TooLong { limit: usize, actual: usize },
}

enum Segment { Literal(String), Input(String) }          // private
pub struct TemplateString { segments: Vec<Segment> }

impl TemplateString {
    pub fn parse(source: &str) -> Result<TemplateString, TemplateError>;   // 문법/prefix
    pub fn input_keys(&self) -> Vec<&str>;                                 // validate용 참조 추출
    pub fn render(&self, inputs: &BTreeMap<String,String>, ctx: SanitizeContext) -> Result<String, TemplateError>;  // 치환+sanitize+length
}
```

`RuntimeContext`에 `inputs` 추가(from_event가 ModalSubmit→inputs, ButtonClick→빈 맵). `Copy` 제거(BTreeMap) → `Clone`. (from_event가 유일 생성자라 안전.)

**RespondEphemeral / ActionSpec / PlannedAction / interpret 시그니처는 무변경**(always-template — content: String 그대로, run에서 렌더).

---

## 4. 파싱 (`${input.<key>}` 만)

- `${` 발견 → `}`까지 읽음. 없으면 `BadSyntax`(unclosed).
- 내부가 `input.<key>` 아니면 `UnsupportedVariable`(예: `${foo.bar}`, `${actor.id}`, `${created.x}`).
- key 비어있으면(`${input.}`) `BadSyntax`.
- 그 외 구간은 `Literal`.

미지원(에러): `${input.name | lower}` · `${input.name ?? "d"}` · `${if...}` · `${created.channel.id}` · `${actor.id}`.

---

## 5. 렌더 (run 시점)

`render(inputs, ctx)`:
1. segment 순회 — Literal은 append, Input(key)은 `inputs.get(key)` (없으면 `MissingInput(key)`).
2. 결과 전체를 `sanitize(ctx)` 통과.
3. sanitize 결과 길이 > limit(ephemeral 2000자)면 `TooLong`.
4. Ok(sanitized).

**static content도 동일 경로** — 변수 없어도 파싱→리터럴→sanitize. AI가 만든 `@everyone 인증 완료`류 static도 자동 방어됨.

---

## 6. sanitize — EphemeralMessageContent

- `@everyone` → `@\u{200b}everyone` (ZWSP로 토큰 무력화)
- `@here` → `@\u{200b}here`
- `<@…>`, `<@&…>` → `<@` 앞 무력화(`<\u{200b}@`)
- `<#…>` → `<\u{200b}#`
- 제어문자 제거(단 `\n`은 유지)
- 길이 초과 → error (truncate 아님)
- **마크다운은 16e에서 안 건드림**(멘션·길이·제어문자 위주; 저자 마크다운 의도 보존)

live phase(후속)에서는 `allowed_mentions: none`도 같이 가야 하지만, 16e는 문자열 sanitize 규칙+테스트가 핵심.

---

## 7. validate / run 경계

**validate (설치 시점, 문법+참조):**
- 각 rule의 RespondEphemeral content를 `parse` — 실패면 `BadTemplate`.
- template의 `input_keys` 추출 →
  - trigger가 **ButtonClick**인데 input 참조 있으면 → 실패(`InputTemplateInButtonRule`). (ButtonClick엔 런타임 input 없음.)
  - trigger가 **ModalSubmit{modal}**이면 각 input key가 그 modal의 field key에 존재해야 함 → 없으면 `UnknownTemplateInput`.

**run (런타임, 치환+보안):**
- `RuntimeContext.inputs`로 실제 값 렌더 → missing이면 TemplateError → sanitize → responder 호출.

경계 요약: **문법/참조 가능성 = validate, 실제 값 치환/보안 = run.**

---

## 8. 에러 처리

`TemplateError`는 개념적으로 **core action failure**이지 Discord adapter failure가 아니다. 다만 `run`이 현재 `Result<(), AdapterError>`라, 첫 컷은 매핑하되 **메시지를 명시**한다:
```
TemplateError → AdapterError { kind: BadRequest, message: "template error: missing input room_name" }
```
`AdapterErrorKind::BadRequest` 추가. handle_event/automation-runtime 시그니처 무변경(그래서 runtime 무수정). 후속에서 RunError로 분리 가능.

*Spec note: Template rendering failure is a core action failure, not a Discord adapter failure. If current result typing requires AdapterError, map it temporarily but keep the source message explicit.*

---

## 9. 테스트 (보안 중심)

**template.rs 순수:**
1. static(placeholder 없음) 렌더 = 원문 유지.
2. `${input.room_name}` 치환.
3. 복수 input 치환.
4. missing input → `MissingInput`.
5. 잘못된 prefix(`${foo.bar}`) → `UnsupportedVariable`; unclosed/`${input.}` → `BadSyntax`.
6. `@everyone` → 무력화(결과에 raw `@everyone` 없음).
7. `@here` → 무력화.
8. `<@123>`/`<@&123>` → 무력화.
9. 마크다운(`**bold**`) → **보존**(escape 안 함).
10. 너무 긴 렌더 결과 → `TooLong`.

**validate:**
11. ButtonClick rule이 `${input.x}` 사용 → `InputTemplateInButtonRule`.
12. ModalSubmit rule이 modal에 없는 `${input.ghost}` 사용 → `UnknownTemplateInput`.
13. ModalSubmit rule이 modal field를 정확히 참조 → 통과.
14. 잘못된 문법 content → `BadTemplate`.

**통합(run/handle_event):**
15. static RespondEphemeral 기존 동작 유지(렌더=원문, mock responder 동일).
16. ModalSubmit inputs가 template로 전달·치환되어 responder에 도달.
17. (기존) unknown fields deny 유지 / automation-core ai-gateway 미의존 유지.

---

## 10. 하지 않는 것 (Forbidden — 16f/후속)
CreateChannel · CreateRole · dynamic resource key · dynamic permission overwrite · PostPanel · 스터디룸 생성 · ChannelName/RoleName sanitize(16f) · `${}` escape · `${actor.id}`/파생 변수 · live Discord · DB · AI.

---

## 11. 로드맵 (재정렬)
```
16c✅ Modal core   16d✅ Modal live   16e▶ Template/sanitize (safe surface, RespondEphemeral)
16f   Dynamic CreateChannel/CreateRole (channel_name/role_name sanitizer + bounded create)
16g   StudyRoom workflow
```

---

## 12. Codex 핸드오프 (개요)
1. automation-core `template.rs` 신설(TemplateString/Segment/TemplateError/SanitizeContext, parse/input_keys/render, sanitize). 순수 테스트 다수.
2. event.rs RuntimeContext에 inputs 추가(Copy 제거), from_event 확장.
3. adapter.rs AdapterErrorKind::BadRequest 추가.
4. validate.rs 템플릿 사전검사(BadTemplate/InputTemplateInButtonRule/UnknownTemplateInput). run.rs RespondEphemeral arm이 parse+render+sanitize, TemplateError→AdapterError(메시지 명시).
5. **automation-runtime/automation-state 무수정** — RespondEphemeral 스키마·interpret·PlannedAction 무변경.
6. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. 완료 후 push. **live/토큰 없음.**

## 최종 정리
16e = always-template + context-aware sanitize, RespondEphemeral 표면 한정. 스키마 무변경(content: String을 항상 template로), 문법·참조는 validate가 설치시점에, 치환·보안은 run이 런타임에. static도 sanitize 통과. 동적 생성은 16f. 이 경계가 "template 성공/실패"와 "리소스 생성 성공/실패"를 분리한다.
