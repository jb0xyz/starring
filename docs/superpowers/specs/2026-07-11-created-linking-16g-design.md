# Phase 16g — Created Resource Linking Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16g — 생성된 리소스의 id를 **뒤 action이 타입 안전하게 참조**. StudyRoom 전체가 아니라 그걸 가능케 하는 **created-id linking core**. 순수 Mock.
- **선행**: 16f(dynamic create). live/overwrite/panel은 16h~16j.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. **16g는 순수 코어(Mock)** — automation-core/automation-state만. automation-runtime은 이번엔 seam 변화 없음(create_* 이미 default-unsupported, GrantRole 시그니처 무변).

**목표:** `ModalSubmit → CreateRole → CreateChannel → GrantRole(created role, actor)` — 생성 결과 id를 뒤 action이 **안전하게** 참조하는가.

---

## 0. 범위 확정

**포함:** CreateRole/CreateChannel에 **output key** · run 중 created id를 **RuntimeBindings**에 저장 · **GrantRole이 created role 참조**(RoleRef::Created) · Mock call sequence 검증 · created id 전달 검증.

**제외(→16h/16i/16j):** permission overwrite · private channel · PostPanel · join button · created channel에 메시지 설치 · live · DB · lifecycle/dedup · rollback live · **ChannelRef 소비**(채널 id는 기록만).

---

## 1. 링킹 = typed reference (문자열 템플릿 금지)

**나쁨:** `role: "${created.study_member_role.id}"` (문자열) — role 자리에 channel id 넣어도 모름, 없는 key도 런타임까지, snowflake 타입안전 깨짐, policy 분석 불가.
**좋음:** `role: { created: "study_member_role" }` (타입).

`${created.x.id}` 문자열 변수는 16e 파서가 `input.`만 허용하므로 **자동 UnsupportedVariable**(test 13).

---

## 2. 타입

### automation-state (스키마)
```rust
pub enum RoleRef {                          // untagged serde
    Existing(ResourceKey),                  // JSON: "verified_member"
    Created { created: String },            // JSON: { "created": "study_member_role" }
}

enum ActionSpec {
    GrantRole { role: RoleRef, target: ActionTarget },   // role: ResourceKey → RoleRef (스키마 변경)
    RespondEphemeral {..}, OpenModal {..},
    CreateChannel { key: String, name: String },         // key 추가
    CreateRole { key: String, name: String },
}
```
`#[serde(untagged)]` on RoleRef → `"x"`→Existing, `{created:"x"}`→Created(distinct JSON shape, 모호성 없음). untagged라 deny_unknown_fields는 없음(RoleRef 한정). **기존 GrantRole 구성은 RoleRef::Existing으로 갱신**(fixture/interpret/validate/policy).

### automation-core
```rust
pub enum PlannedRole { Resolved(RoleId), Created(String) }   // Created = action key
enum PlannedAction {
    GrantRole { role: PlannedRole, target: UserId },
    RespondEphemeral {..}, OpenModal(..),
    CreateChannel { key: String, name: String },
    CreateRole { key: String, name: String },
}
#[derive(Default)]
pub struct RuntimeBindings {
    pub created_roles: BTreeMap<String, RoleId>,
    pub created_channels: BTreeMap<String, ChannelId>,
}
```

---

## 3. interpret / run 경계

- **interpret**(설치 binding 있음, created id는 없음):
  - GrantRole{role: Existing(key)} → `bindings.role_bindings` 해소 → `PlannedRole::Resolved(RoleId)`(16a 방식). 미해소 시 None.
  - GrantRole{role: Created(action_key)} → 해소 불가(런타임 생성) → `PlannedRole::Created(action_key)`.
  - CreateRole/CreateChannel{key, name} → PlannedAction에 key 유지.
- **run**(RuntimeBindings 유지):
  1. CreateRole{key,name} → name 렌더(RoleName) → create_role → id → `bindings.created_roles[key]=id` + CreatedResource 기록.
  2. CreateChannel{key,name} → 렌더(ChannelName) → create_channel → `bindings.created_channels[key]=id` + 기록.
  3. GrantRole{role, target}: `PlannedRole::Resolved(id)` → 그 id / `PlannedRole::Created(key)` → `bindings.created_roles.get(key)`(validate 보장, 없으면 방어적 에러) → grant_role(guild, target, id).

fail-fast: CreateRole/CreateChannel 실패 시 뒤 GrantRole 미실행(기존 `?` 전파).

---

## 4. validate (order 기반)

action을 **순서대로** 훑으며 `created: Map<key → Role|Channel>` 누적:
- CreateRole/CreateChannel: key가 이미 있으면 **DuplicateActionKey**. 없으면 등록.
- GrantRole{role: Created(key)}: `created.get(key)` —
  - None(미정의 또는 **자기보다 뒤**) → **UnknownCreatedRoleRef**(forward ref 포함).
  - Some(Channel) → **CreatedRoleRefTypeMismatch**(역할 자리에 채널 key).
  - Some(Role) → ok.
- GrantRole{role: Existing(key)}: `bindings.role_bindings` 확인(16a UnknownRoleRef).
- (16f) CreateRole/CreateChannel name 템플릿 검사(check_template) 유지.

**forward ref 금지**가 핵심 — MVP는 순서 실행이라 created ref는 앞선 action만 참조 가능. (action graph는 후속.)

---

## 5. policy (notice)

created linking 자체는 차단이 아니라 **preview/audit용 notice**. PolicyFinding(16f enum)에 추가:
```rust
    CreatedResourceReference { rule: String },   // "이 rule은 같은 실행에서 만든 역할을 부여한다"
```
GrantRole{role: Created(..)} → 이 finding. (dynamic create finding은 16f 그대로.)

---

## 6. StudyRoom MVP (16g — 비공개 아님, 뼈대)
```
rules:
  - key: create_study_room
    trigger: { modal_submit, create_study_modal }
    actions:
      - { create_role,    key: study_member_role, name: "${input.room_name} 멤버" }
      - { create_channel, key: study_channel,      name: "study-${input.room_name}" }
      - { grant_role, role: { created: study_member_role }, target: actor }
      - { respond_ephemeral, content: "스터디룸이 생성되었습니다." }
```
증명: 입력 기반 생성 + created role id 저장 + 그 id로 GrantRole. (created channel id는 기록만 — 16h/16i에서 overwrite/panel.)

---

## 7. 테스트 (13)
1. CreateRole key가 RuntimeBindings에 저장.
2. GrantRole이 created role key 해소 → actor 지급.
3. CreateChannel key가 binding에 저장.
4. action key 중복 → validate 실패.
5. Created role ref가 없는 key → validate 실패.
6. RoleRef::Created가 CreateChannel key 참조 → validate 실패(type mismatch).
7. forward ref(뒤 action 참조) → validate 실패.
8. created role id == MockAdapter 반환 id 일치.
9. call sequence: create_role → create_channel → grant_role.
10. create_role 실패 → grant_role 미실행.
11. create_channel 실패 → grant_role 미실행.
12. created channel id 기록되지만 16g에서 소비 안 해도 됨.
13. `${created.x.id}` 템플릿 변수 → UnsupportedVariable(파서 자동 거부).

---

## 8. 하지 않는 것 (Forbidden — 16h/i/j)
permission overwrite · private channel · PostPanel · join button · created channel 메시지 설치 · ChannelRef 소비 · live · DB · dedup · rollback live · action graph(순서 실행만).

---

## 9. 로드맵 (재정렬)
```
16f✅ Dynamic Create   16g▶ Created linking (이 스펙)
16h  Private channel overwrite core   16i  PostPanel core   16j  StudyRoom live smoke
```

---

## 10. Codex 핸드오프 (개요)
1. automation-state: RoleRef(untagged) + ActionSpec(GrantRole.role→RoleRef, CreateRole/CreateChannel에 key). 기존 GrantRole fixture 갱신.
2. automation-core: plan.rs(PlannedRole + PlannedAction key/role 변경), event.rs? (RuntimeBindings는 run 로컬), run.rs(RuntimeBindings 유지 + Created 해소 + create가 binding 채움), interpret.rs(Existing 해소/Created 보존/create key 유지), validate.rs(order 기반 key/ref 검사), policy.rs(CreatedResourceReference).
3. **automation-runtime 무수정**(GrantRole은 여전히 grant_role(guild,member,role) seam; created 해소는 core에서). 확인.
4. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. 완료 후 push. **live/토큰 없음.**

## 최종 정리
16g = created-id linking core. 생성 결과 참조는 **typed RoleRef**(Existing/Created), 문자열 템플릿 금지(`${created}` 자동거부). CreateRole/CreateChannel에 key, run이 RuntimeBindings에 저장, GrantRole이 created role 참조. validate가 key 중복·타입·forward ref 차단. 이걸로 16h(overwrite)·16i(panel)·16j(live)로 StudyRoom 조립 준비.
