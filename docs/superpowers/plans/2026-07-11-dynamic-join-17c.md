# Phase 17c — Dynamic Join Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** instance-scoped 버튼 → `InstanceAction` 이벤트 → 룰(`TriggerSpec::InstanceAction`) → `RoleRef::Instance{event, alias}`로 기존 `GrantRole` 조립. + AutomationServices bundle. 순수 Mock.

**Architecture:** join은 특별 기능 아님 — 확장된 typed-ref 조립. custom_id 4-seg(registry 인디렉션), handle_event가 instance 1회 해소→RuntimeContext.instance snapshot, GrantRole가 그 snapshot의 alias 역할 지급.

## Global Constraints
- **코드 주석 금지.** **Codex 구현.**
- **serde 실증됨**: InstanceRef 커스텀(Event→`"event"`), RoleRef untagged 3변형.
- 안전 불변식: guild+id 조회만·Active만·ruleset 일치·alias는 roles map만·missing/deleted=typed rejection(silent no-op 금지).
- 게이트 build/test/clippy(-D warnings)/fmt. push. **live 없음(17e).**

---

## Task A: AutomationServices bundle 리팩터 (순수)

- [ ] **Step 1: `automation-core/adapter.rs`(또는 신규 services.rs) — AutomationServices**

```rust
pub struct AutomationServices<'a, M, R, S, G>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: automation_instance::InstanceStore,
    G: automation_instance::InstanceIdGenerator,
{
    pub mutation: &'a M,
    pub responder: &'a R,
    pub instances: &'a S,
    pub instance_ids: &'a G,
}
```
lib.rs 재노출.

- [ ] **Step 2: `run.rs` — run/handle_event가 services 받도록**

`run(context, plan, mutation, responder, instances, instance_ids)` → `run(context, plan, services: &AutomationServices<...>)`. 내부 `mutation.` → `services.mutation.`, `responder.` → `services.responder.`, `instances.` → `services.instances.`, `instance_ids.` → `services.instance_ids.`. handle_event도 `services` + `failure_message` + `ruleset_key`만(9→서비스 묶음). `#[allow(too_many_arguments)]` 제거.

- [ ] **Step 3: 호출자 갱신(컴파일러 가이드)**

run/handle_event 호출 전부: 개별 인자 → `&AutomationServices{mutation, responder, instances, instance_ids}` 구성. runner도. 대상 automation-core tests 다수 + automation-runtime runner + tool.

- [ ] **Step 4: 게이트 + 커밋**

`cargo build`(경고0) + `cargo test`(기존 325 유지) + clippy/fmt:
```bash
git add -A && git commit -m "refactor(automation-core): AutomationServices bundle"
```

---

## Task B: state 타입 (InstanceRef/RoleRef::Instance/InstanceAction/ButtonRoute)

- [ ] **Step 1: `automation-state/rule.rs` — InstanceRef + RoleRef::Instance**

`InstanceRef`(커스텀 serde — 실증됨):
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceRef {
    Event,
    Created(CreatedRef),
}

impl serde::Serialize for InstanceRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            InstanceRef::Event => serializer.serialize_str("event"),
            InstanceRef::Created(created) => created.serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for InstanceRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InstanceRefVisitor;
        impl<'de> serde::de::Visitor<'de> for InstanceRefVisitor {
            type Value = InstanceRef;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(r#""event" or { "created": <key> }"#)
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<InstanceRef, E> {
                if value == "event" {
                    Ok(InstanceRef::Event)
                } else {
                    Err(E::custom("expected \"event\""))
                }
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<InstanceRef, A::Error> {
                let created = CreatedRef::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(InstanceRef::Created(created))
            }
        }
        deserializer.deserialize_any(InstanceRefVisitor)
    }
}
```

`RoleRef`에 Instance 변형 추가(16g untagged):
```rust
#[serde(untagged)]
pub enum RoleRef {
    Existing(ResourceKey),
    Created(CreatedRef),
    Instance {
        instance: InstanceRef,
        alias: String,
    },
}
```

- [ ] **Step 2: `rule.rs` — TriggerSpec::InstanceAction + ButtonRoute + ButtonSpec route**

`TriggerSpec`에:
```rust
    InstanceAction { action: String },
```
(내부태그 deny_unknown_fields 유지.)

`panel.rs`의 `ButtonSpec`을 `{label, route}`로 일반화:
```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonSpec {
    pub label: String,
    pub route: ButtonRoute,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ButtonRoute {
    Static { key: String },
    InstanceAction { instance: crate::rule::InstanceRef, action: String },
}
```
> **파급**: 기존 `ButtonSpec{key, label}` 구성 전부 `ButtonSpec{label, route: ButtonRoute::Static{key}}`로(컴파일러 가이드). button key 추출(16i validate 전역 button_keys, 16b/d custom_id)은 `ButtonRoute::Static{key}`에서.

lib.rs 재노출(InstanceRef, ButtonRoute).

- [ ] **Step 3: 파급 + serde 테스트**

`ButtonSpec` 구성 전부 갱신(rule.rs 테스트/tests/*). RoleRef 3변형 + InstanceRef + ButtonRoute + InstanceAction trigger serde roundtrip 테스트. `cargo test -p automation-state` → 커밋 `feat(automation-state): instance role ref + button route + instance_action trigger`

---

## Task C: core (EventKind/RuntimeContext.instance/interpret/run/validate) + custom_id

- [ ] **Step 1: `custom_id.rs`(automation-runtime) — 4-seg instance action**

```rust
const INSTANCE: &str = "i";
pub fn encode_instance_action(instance_id: &str, action: &str) -> String {
    format!("{PREFIX}:{INSTANCE}:{instance_id}:{action}")
}
```
decode 확장: `parts[1] == "i"`면 InstanceAction(instance_id=parts[2], action=parts[3], 4-seg). 기존 5-seg(button/modal) 유지. `ParsedCustomId`에 InstanceAction 케이스(enum 확장 또는 별도 함수).

- [ ] **Step 2: `event.rs` — EventKind::InstanceAction + RuntimeContext.instance**

```rust
enum EventKind {
    ButtonClick { component: String },
    ModalSubmit { modal: String, inputs: BTreeMap<String, String> },
    InstanceAction { instance_id: automation_instance::InstanceId, action: String },
}

struct RuntimeContext {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub ruleset_key: String,
    pub inputs: BTreeMap<String, String>,
    pub instance: Option<ResolvedInstanceContext>,
}
pub struct ResolvedInstanceContext {
    pub instance: automation_instance::AutomationInstance,
    pub action: String,
}
```
`from_event`는 instance=None로 두고, handle_event가 InstanceAction일 때 해소해 채움(Step 4).

- [ ] **Step 3: `interpret.rs` — InstanceAction trigger match + GrantRole RoleRef::Instance pass**

`trigger_matches`에 `(TriggerSpec::InstanceAction{action}, EventKind::InstanceAction{action: a, ..}) => action == a` 추가. GrantRole arm: RoleRef::Instance는 interpret에서 해소 못 함(런타임 instance 필요) → PlannedRole에 Instance 케이스 추가하거나, GrantRole을 PlannedAction으로 옮길 때 RoleRef::Instance{alias}를 운반. **결정: `PlannedRole`에 `Instance{alias: String}` 추가**(Event 암묵 — context.instance 사용). interpret: `RoleRef::Instance{instance: InstanceRef::Event, alias}` → `PlannedRole::Instance{alias}`. (`InstanceRef::Created`는 17c GrantRole에선 미사용 — 버튼 route 전용; validate가 GrantRole의 instance는 Event만 허용.)

- [ ] **Step 4: `run.rs`/`handle_event` — instance 해소 + GrantRole Instance arm**

handle_event: InstanceAction 이벤트면 `services.instances.get(guild, instance_id)` → status Active·ruleset 일치 검증(아니면 typed rejection) → `context.instance = Some(ResolvedInstanceContext{instance, action})`. run의 GrantRole `PlannedRole::Instance{alias}` 해소: `context.instance.as_ref().resources.roles.get(alias)` → RoleId, 없으면 `InstanceResourceNotFound`(BadRequest). roles map만.

- [ ] **Step 5: `validate.rs` — InstanceAction rule + RoleRef::Instance**

InstanceAction trigger 룰: GrantRole의 RoleRef::Instance는 `instance: Event`만 허용(Created는 error) + alias 규칙(1~32/`[a-zA-Z0-9_-]`). RoleRef::Instance를 non-InstanceAction 룰에서 쓰면 error(InstanceRoleOutsideInstanceRule). policy.rs no-op arm.

- [ ] **Step 6: 파급 + 테스트 + 커밋**

호출자/CreatedResource/EventKind 파급(컴파일러 가이드) + `tests/dynamic_join.rs`(스펙 §8: encode/decode, Active join, Disabled/Deleted/missing/ruleset 거부, alias 해소/missing, roles-map-only, static 유지, InstanceRef::Created 버튼 custom_id, RoleRef::Instance{Event} GrantRole). `cargo build/test -p automation-core` → 커밋.

---

## Task D: runtime convert/버튼 route + tool + 전체 게이트 + push

- [ ] **Step 1: `automation-runtime/convert.rs` — instance custom_id → InstanceAction event**

decode가 InstanceAction(4-seg)이면 `EventKind::InstanceAction{instance_id: InstanceId::parse(...)?, action}`. instance action은 ruleset_key 가드가 다름(instance에서 ruleset 해소는 handle_event) — convert는 guild + instance_id + action만.

- [ ] **Step 2: `automation-runtime` PostPanel 버튼 route 인코딩**

post_panel이 버튼을 게시할 때 `ButtonRoute::Static{key}` → `encode_button(guild, ruleset_key, key)`, `ButtonRoute::InstanceAction{instance, action}` → instance 해소(created_instances binding, run이 전달) → `encode_instance_action(instance_id, action)`. (버튼 route 해소는 run/post_panel 경로 — PostPanelSpec 확장 or PlannedAction에 resolved custom_id.)

- [ ] **Step 3: `tools/interaction-smoke` — 허브 join 버튼 + join 룰**

StudyRoom run에 허브 PostPanel(참가 버튼 route: InstanceAction{instance:{created:study_room_instance}, action:join}) + join 룰(trigger instance_action join → GrantRole{role:{instance:event, alias:member_role}} + respond_ephemeral).

- [ ] **Step 4~6: 전체 게이트 + push**
- build(경고0)/test(전체 ~345)/clippy(-D warnings)/fmt/`git push origin main`. 커밋: `feat(interaction-smoke): dynamic join wiring`.

---

## Self-Review (스펙 대비)
- RoleRef::Instance(GrantRole 재사용) + InstanceRef(커스텀 serde 실증) + ButtonRoute(PostPanel 버튼) + TriggerSpec/EventKind::InstanceAction ✅.
- custom_id 4-seg(registry 인디렉션) + handle_event 1회 해소→RuntimeContext.instance(status/ruleset 검증) ✅.
- AutomationServices bundle(9인자 제거) ✅. 안전 불변식(guild+id/Active/ruleset/roles-map/typed rejection) ✅.
- ButtonSpec {key,label}→{label,route} 파급 컴파일러 가이드 ✅.

## Codex 핸드오프 (4청크)
- **A** = AutomationServices bundle 리팩터(기존 325 유지). 커밋 1.
- **B** = state 타입(InstanceRef/RoleRef::Instance/InstanceAction/ButtonRoute+ButtonSpec 파급) + serde 테스트. 커밋 1.
- **C** = core(EventKind/RuntimeContext.instance/interpret/run/validate + custom_id) + 테스트. 커밋 1.
- **D** = runtime convert/버튼 route + tool + 전체 게이트 + push. 커밋 1 + push.
보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + 파급 규모 + 이탈. **live 없음.**
