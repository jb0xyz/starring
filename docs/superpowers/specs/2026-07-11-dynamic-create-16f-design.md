# Phase 16f — Dynamic CreateChannel / CreateRole Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16f — 사용자 입력값으로 **안전한 이름**을 만들고 그 이름으로 채널/역할 생성 action 실행. 순수 코어 + Mock.
- **선행**: 16e(template/sanitize 엔진). live는 16g.

---

## ⚠️ 최상위 원칙 (불변)
AI는 설치시점 설계자, Runtime은 결정론적 interpreter, 이벤트-타임 LLM 금지. **16f는 순수 코어(Mock)** — automation-core/automation-state만 수정, **automation-runtime 무수정**(새 seam은 default-unsupported).

**목표:** 입력값으로 안전한 이름 → 채널/역할 생성 action. **생성 id는 기록만, 다음 action이 참조 못 함.** 권한/overwrite/링킹/live는 16g.

---

## 0. 확정 결정 (Ⓐ)

**포함:** `ActionSpec::CreateChannel { name }` + `CreateRole { name }`(name=always-template) · ChannelName/RoleName sanitizer · DiscordMutationAdapter create_channel/create_role seam · Mock 실행 · created id 기록 · dynamic-create policy finding.

**제외(→16g):** permissions · overwrites · private channels · created id 링킹 · grant created role · post panel into created channel · live · DB/lifecycle/dedup.

---

## 1. 핵심 경계

1. **created id는 "기록만".** adapter가 id 반환 → run이 `CreatedResource`로 기록. 하지만 **16f action은 created id를 참조 못 함.** `${created.channel.id}` 같은 placeholder는 16e 파서가 `input.`만 허용하므로 **UnsupportedVariable로 자동 거부**(16f = capture, 16g = linking).
2. **CreateRole은 권한 없는 마커.** `CreateRole { name }`, 내부 permissions=empty 고정. 권한 상승 수단 아님.
3. **CreateChannel은 공개 기본 텍스트 채널.** `CreateChannel { name }`, channel_type=Text, overwrites 없음. 비공개/권한은 16g.
4. **중복 이름 처리 없음.** 순수 코어라 실제 서버와 비교 안 함. name을 결과에 기록(후속 policy/audit용). dedup은 16g/DB.

---

## 2. 타입

### automation-state (스키마 확장, deny_unknown_fields)
```rust
enum ActionSpec {
    GrantRole {..}, RespondEphemeral {..}, OpenModal {..},
    CreateChannel { name: String },   // name = always-template
    CreateRole { name: String },
}
```
```
{ "type": "create_channel", "name": "study-${input.room_name}" }
{ "type": "create_role", "name": "${input.room_name} 멤버" }
```

### automation-core
```rust
pub struct CreateChannelSpec { pub name: String }   // 16g에서 parent_id/overwrites 추가 여지
pub struct CreateRoleSpec { pub name: String }       // 16g에서 permissions 추가 여지

#[allow(async_fn_in_trait)]
pub trait DiscordMutationAdapter {
    async fn grant_role(...) -> Result<(), AdapterError>;
    async fn create_channel(&self, guild: GuildId, spec: CreateChannelSpec) -> Result<ChannelId, AdapterError> {
        let _ = (guild, spec);
        Err(AdapterError::new(AdapterErrorKind::Unsupported, "create_channel is not supported"))
    }
    async fn create_role(&self, guild: GuildId, spec: CreateRoleSpec) -> Result<RoleId, AdapterError> {
        let _ = (guild, spec);
        Err(AdapterError::new(AdapterErrorKind::Unsupported, "create_role is not supported"))
    }
}

enum PlannedAction {
    GrantRole {..}, RespondEphemeral {..}, OpenModal(..),
    CreateChannel { name: String },   // raw template, run에서 렌더
    CreateRole { name: String },
}

pub enum CreatedResource {
    Channel { action_index: usize, name: String, id: ChannelId },
    Role { action_index: usize, name: String, id: RoleId },
}

enum SanitizeContext { EphemeralMessageContent, ChannelName, RoleName }   // 확장
enum TemplateError { BadSyntax, UnsupportedVariable, MissingInput, TooLong, EmptyAfterSanitize }   // 추가
```
default-unsupported seam → **automation-runtime TwilightMutationAdapter 무수정 컴파일**(16c open_modal 패턴). Mock override, 실구현 16g.

---

## 3. sanitizer 2종

`sanitize(input, ctx) -> Result<String, TemplateError>`(EmptyAfterSanitize 추가). max_len: Ephemeral 2000, **Channel/Role 100**.

**ChannelName (가장 보수적 — ASCII slug, 한글 제거 = A안):**
- lower-case · a-z/0-9 유지 · 그 외(공백·언더스코어·구두점·**비ASCII**)는 `-`로(연속 collapse) · 앞뒤 `-` 제거 · 결과 비면 **EmptyAfterSanitize** · 100자 초과 TooLong.
- `"Study Room 1"` → `"study-room-1"` / `"수학 스터디"` → `""` → error(A안: 한글 slugify는 후속 display-name 분리로).

**RoleName (덜 빡빡 — 한글 유지):**
- trim · 제어문자 제거 · `@everyone`/`@here`/`<@`/`<#` ZWSP 무력화 · 공백 collapse · 결과 비면 EmptyAfterSanitize · 100자 초과 TooLong.
- `"수학 스터디 멤버"` → 유지 / `"@everyone 멤버"` → 무력화.

**empty → error**(자동 fallback 금지 — 의도 안 한 이름 방지). fallback 정책은 후속.

---

## 4. render / run

- interpret: CreateChannel/CreateRole → `PlannedAction::CreateChannel/CreateRole { name: raw template }`(16e처럼 interpret은 렌더 안 함).
- run: 각 action을 **맞는 context로 렌더**:
  - CreateChannel.name → render(ChannelName) → `create_channel(guild, CreateChannelSpec{name})` → id → `CreatedResource::Channel` 기록.
  - CreateRole.name → render(RoleName) → `create_role(...)` → `CreatedResource::Role` 기록.
  - RespondEphemeral.content → render(EphemeralMessageContent)(16e).
- **`run(...) -> Result<Vec<CreatedResource>, AdapterError>`** — 생성 순서대로 기록. `handle_event`는 이 Vec를 discard(HandleOutcome 무변경 → 기존 테스트 유지). created-id 확인은 run 직접 호출.

---

## 5. validate

CreateChannel.name / CreateRole.name도 RespondEphemeral.content와 **같은 템플릿 검사**(16e): parse(문법=BadTemplate) · input_keys 추출 · ButtonClick rule이 input 참조 → InputTemplateInButtonRule · ModalSubmit rule은 modal field 존재 확인 → UnknownTemplateInput. (context sanitize는 run; validate는 문법·참조만.)

---

## 6. policy (dynamic-create 위험 표시 — 최소)

dynamic create는 설치 후 매 제출마다 리소스를 만들 수 있어 단순 응답보다 위험도가 높다. live 없어도 policy 모델에 표시해야 16g가 안전.

`PolicyFinding`을 enum으로:
```rust
pub enum PolicyFinding {
    PrivilegedRoleGrant { rule: String, role: ResourceKey },   // 16a
    DynamicResourceCreation { rule: String, action: DynamicAction },   // 16f
}
pub enum DynamicAction { CreateChannel, CreateRole }
```
`analyze`가 CreateChannel/CreateRole → `DynamicResourceCreation` flag. **기존 16a policy 테스트 2개는 enum 매칭으로 갱신.**

---

## 7. 테스트 (20)

**template.rs sanitizer 순수:**
- (3) ChannelName spaces→hyphens / (4) uppercase→lowercase / (5) invalid chars 제거 / (6) empty→EmptyAfterSanitize / (7) len>100→TooLong.
- (8) RoleName 한글 유지 / (9) @everyone 무력화 / (10) len>100→TooLong.

**interpret/run:**
- (1) CreateChannel name template 렌더 / (2) CreateRole name template 렌더.
- (11) CreateChannel name missing input→error / (12) CreateRole name missing input→error.
- (14) Mock create_channel 기록 / (15) Mock create_role 기록.
- (16) created id가 run 결과(Vec<CreatedResource>)에 기록.

**validate/policy:**
- (13) ButtonClick의 create action에서 `${input.x}` → InputTemplateInButtonRule.
- (17) `${created.channel.id}` → UnsupportedVariable(16e 파서 자동 거부).
- (18) policy가 CreateChannel flag / (19) CreateRole flag.
- (20) 기존 RespondEphemeral 템플릿 테스트 전부 유지.

---

## 8. 하지 않는 것 (Forbidden — 16g/후속)
permissions · overwrites · private channel · created id 링킹(`${created.x}`) · grant created role · post panel · live · DB · dedup · 한글 slug/display-name 분리.

---

## 9. 로드맵
```
16e✅ Template/sanitize   16f▶ Dynamic Create (이 스펙)   16g StudyRoom(링킹+권한+overwrite+live)
```

---

## 10. Codex 핸드오프 (개요)
1. automation-state: ActionSpec::CreateChannel/CreateRole(name), deny_unknown_fields.
2. automation-core: template.rs(SanitizeContext ChannelName/RoleName + sanitize fallible + EmptyAfterSanitize + sanitizer 2종), adapter.rs(CreateChannelSpec/CreateRoleSpec + seam default-unsupported), plan.rs(PlannedAction 2종 + CreatedResource), mock.rs(create_* override + MutationCall 2종), interpret.rs(2 action arm), run.rs(렌더+create+Vec 반환), validate.rs(name 템플릿 검사), policy.rs(PolicyFinding enum + DynamicResourceCreation).
3. **automation-runtime 무수정**(default-unsupported seam으로 컴파일 유지) — 반드시 확인.
4. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. 완료 후 push. **live/토큰 없음.**

## 최종 정리
16f = 입력값 → 안전한 이름 → 채널/역할 생성(마커 역할·공개 채널). created id 기록만(링킹 금지, 16e 파서가 `${created}` 자동 차단). ChannelName은 ASCII slug(한글 제거·empty error), RoleName은 한글 유지+멘션 무력화. policy가 dynamic-create 위험 표시. 이걸로 16g 스터디룸(링킹+권한+overwrite+live) 조립 준비 완료.
