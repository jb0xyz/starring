# Phase 17c — Dynamic Join Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 17c — 등록된 instance를 참조하는 **instance-scoped 버튼 + 이벤트 + typed instance role ref**로 join(및 leave/close/claim 등)을 기존 primitive 조립으로 처리. + AutomationServices bundle. 순수 Mock.
- **선행**: 17a(registry) + 17b(registration: AutomationInstance/InstanceStore/RegisterInstance/created_instances binding).

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, **event-time LLM 금지**. no_ai_gateway 유지. **join은 특별 기능 아님** — instance-scoped 버튼이 `InstanceAction` 이벤트를 내고, 룰이 `RoleRef::Instance`로 참조해 기존 `GrantRole`을 조립. runtime에 join/role-alias 하드코딩 금지.

**목표(한 문장):** instance 버튼 클릭 → custom_id로 instance_id 해소 → handle_event가 (guild+id)로 Store 조회·status·ruleset 검증 후 `RuntimeContext.instance`에 snapshot → 룰이 `RoleRef::Instance{event, alias}`로 그 instance의 역할을 actor에 지급.

---

## 0. 범위

**포함:** `EventKind::InstanceAction` + `TriggerSpec::InstanceAction{action}` + **`RoleRef::Instance{instance, alias}`**(기존 GrantRole 재사용) + `InstanceRef{Event, Created}` + PostPanel 버튼 **route**(`ButtonRoute{Static, InstanceAction}`) + custom_id 4-seg(`starring:i:<id>:<action>`) + handle_event instance 해소→`RuntimeContext.instance` + **AutomationServices bundle** + validate + 안전 불변식 + Mock 테스트.

**제외(→17d/17e):** DB · live · Disabled/Deleted 사용자 메시지(core는 typed rejection까지) · leave/close/claim 등 다른 action(같은 엔진이라 자동으로 되지만 시나리오는 join만) · `TriggerSpec::InstanceAction.kind`(여지만).

---

## 1. join = 특별 기능이 아님 (일반화)
`GrantInstanceRole`/`PostJoinPanel` 같은 join 전용 액션을 만들면 곧 `PostToInstanceChannel`/`RevokeInstanceRole`/... 기능 모음집이 됨. 대신 **참조를 확장**:
- 버튼은 instance-scoped route → `InstanceAction(instance_id, action)` 이벤트.
- 룰이 그 이벤트에 반응(trigger) → `RoleRef::Instance`로 instance 역할 참조 → 기존 GrantRole.
그러면 join·leave·close·claim·approve가 전부 같은 primitive 위에.

---

## 2. 타입 (automation-state)

```rust
enum InstanceRef {                 // untagged: "event" | { "created": key }
    Event,
    Created(CreatedRef),
}

enum RoleRef {                     // 16g 확장 (untagged)
    Existing(ResourceKey),         // "verified"
    Created(CreatedRef),           // { "created": "study_member_role" }
    Instance {                     // { "instance": <InstanceRef>, "alias": "member_role" }
        instance: InstanceRef,
        alias: String,             // InstanceResourceKey 규칙(1~32, [a-zA-Z0-9_-]) — validate
    },
}

enum TriggerSpec {
    ButtonClick { component },
    ModalSubmit { modal },
    InstanceAction { action: String },   // kind: Option<InstanceKind>는 여지(17c 생략)
}

// PostPanel 버튼: route 도입
enum ButtonRoute {
    Static { key: String },                                  // 기존 정적 버튼
    InstanceAction { instance: InstanceRef, action: String },
}
struct ButtonSpec {                // {label, route}로 일반화(기존 {key,label} → route)
    label: String,
    route: ButtonRoute,
}
```
> **serde 리스크(플랜 실증)**: RoleRef untagged 3변형(bare/`{created}`/`{instance,alias}`) + InstanceRef untagged(`"event"`/`{created}`) — 16g처럼 scratch 실증. ButtonSpec `{key,label}`→`{label,route}` 변경은 파급(기존 버튼 구성 전부).

DSL 예:
```yaml
# join 룰
- key: study_join_rule
  trigger: { instance_action, action: join }
  actions:
    - { grant_role, role: { instance: event, alias: member_role }, target: actor }
    - { respond_ephemeral, content: "스터디룸에 참가했습니다." }

# 공개 허브에 join 버튼(17b run 흐름 안)
- { post_panel, key: study_hub_entry, channel: {existing: study_hub},
    content: "'${input.room_name}' 스터디룸이 열렸습니다.",
    buttons: [ { label: 참가하기, route: { instance_action: { instance: {created: study_room_instance}, action: join } } } ] }
```

---

## 3. custom_id 4-segment
`starring:i:<instance_id>:<action>` (예: `starring:i:room_a82k4:join`). **guild는 interaction에서, ruleset_key는 registry instance에서** 해소 → 반복 제거, 100자 회피. custom_id.rs에 encode_instance_action/decode 추가(기존 5-seg button/modal 유지).

---

## 4. handle_event instance 해소 (1회 전처리)
```
1. custom_id decode → instance_id + action (InstanceAction)
2. InstanceStore.get(interaction.guild_id, instance_id)   ← guild+id로만
3. status == Active 아니면 typed rejection(mutation 금지)
4. instance.ruleset_key == 실행 ruleset_key 아니면 rejection
5. RuntimeContext.instance = Some(ResolvedInstanceContext{instance, action})
6. EventKind::InstanceAction{instance_id, action} → trigger match
7. action 실행(GrantRole가 context.instance로 alias 해소)
```
```rust
struct ResolvedInstanceContext { instance: AutomationInstance, action: String }
struct RuntimeContext {
    guild_id, actor, ruleset_key, inputs,
    instance: Option<ResolvedInstanceContext>,   // InstanceAction일 때만 Some
}
```
- **Store 조회 1회** — 여러 action(role 지급 + channel 안내 + welcome 수정 + audit)이 같은 snapshot 재사용.
- run의 GrantRole arm에서 `RoleRef::Instance{Event, alias}` → `context.instance.resources.roles[alias]` → RoleId. **없으면 typed error**(`InstanceResourceNotFound{instance_id, Role, alias}`), silent no-op 금지. **roles map에서만**(channel/message id를 role로 못 씀).

---

## 5. AutomationServices bundle (지금 도입)
```rust
struct AutomationServices<'a, M, R, S, G> {
    mutation: &'a M,
    responder: &'a R,
    instances: &'a S,
    instance_ids: &'a G,
}
```
run/handle_event가 개별 인자(9개) 대신 `services: &AutomationServices<...>` 받음. **책임 분리**: Services=외부 seam/capability, RuntimeContext=이번 interaction 데이터(instance snapshot 포함). 17b `#[allow(too_many_arguments)]` 제거. 호출자 전부 갱신(컴파일러 가이드).

---

## 6. 안전 불변식 (build/test 고정)
- InstanceStore 조회는 **GuildId + InstanceId로만**(custom_id 신뢰 안 함).
- status != Active → mutation 금지(typed rejection).
- instance.ruleset_key ≠ 실행 ruleset → 실행 금지.
- role alias 없음 → mutation 금지(typed error, silent no-op 금지).
- role alias는 **roles map에서만** 조회. channel/message id를 role로 사용 불가.
- missing/deleted instance → typed rejection.
- event-time LLM 없음.

---

## 7. 로드맵 / 청킹
```
17a✅ Registry   17b✅ Registration   17c▶ Dynamic Join Core (이 스펙)
17d PostgreSQL InstanceStore(별도 crate)   17e Durable Dynamic Join Live
```
**청킹(17b처럼 다중):** A) AutomationServices bundle 리팩터(순수 리팩터, 기존 테스트 유지) · B) state 타입(InstanceRef/RoleRef::Instance/TriggerSpec::InstanceAction/ButtonRoute+ButtonSpec) + serde 실증 · C) core(EventKind::InstanceAction, RuntimeContext.instance, interpret/run GrantRole-instance/validate) + custom_id 4-seg · D) runtime(convert instance custom_id + 버튼 route 인코딩) + tool(허브 join 버튼 + join 룰) + 전체 게이트.

---

## 8. 테스트 (핵심 19)
1. instance custom_id encode/decode(4-seg) + 길이 검증. 2. guild+id 조회. 3. 다른 guild 격리. 4. Active join 성공(actor에 해소 RoleId 지급). 5. Disabled 거부. 6. Deleted 거부. 7. missing instance 거부. 8. ruleset_key mismatch 거부. 9. role alias 정상 해소. 10. missing alias → typed error. 11. role/channel/message 타입 혼동 불가(alias는 roles map만). 12. 추가 RespondEphemeral 조합. 13. static button routing 기존 유지. 14. InstanceAction이 하드코딩 없이 룰 매칭. 15. InstanceRef::Created로 동적 버튼 custom_id 생성. 16. RoleRef::Instance{Event}가 기존 GrantRole에서 작동. 17. AutomationServices bundle로 기존 325 semantics 유지. 18. RoleRef serde 3변형 실증. 19. no_ai_gateway.

---

## 9. Codex 핸드오프 (개요)
1. custom_id: encode_instance_action/decode(4-seg).
2. automation-state: InstanceRef + RoleRef::Instance + TriggerSpec::InstanceAction + ButtonRoute + ButtonSpec{label,route}(파급) + serde 테스트.
3. automation-core: AutomationServices bundle(run/handle_event) + EventKind::InstanceAction + RuntimeContext.instance + interpret(InstanceAction trigger, GrantRole-instance) + run(GrantRole RoleRef::Instance 해소 + typed rejection) + handle_event instance 해소/status/ruleset 검증 + validate(InstanceAction rule, alias, roles-map-only) + 호출자 갱신.
4. automation-runtime: convert(instance custom_id → InstanceAction event) + PostPanel 버튼 route 인코딩(InstanceRef::Created → custom_id) + gateway/runner services 배선.
5. tool: 허브 join 버튼(route) + join 룰. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push. **live 없음(17e).**

## 최종 정리
17c = Dynamic Join Core. join은 특별 기능이 아니라 **instance-scoped 버튼(ButtonRoute::InstanceAction) → InstanceAction 이벤트 → 룰(TriggerSpec::InstanceAction) → RoleRef::Instance{event, alias}로 기존 GrantRole 조립**. custom_id 4-seg(registry 인디렉션). handle_event가 guild+id로 instance 1회 해소·status/ruleset 검증→RuntimeContext.instance snapshot. AutomationServices bundle. typed rejection(silent no-op 금지). 이 엔진 위에서 join/leave/close/claim이 전부 룰로 표현됨.
