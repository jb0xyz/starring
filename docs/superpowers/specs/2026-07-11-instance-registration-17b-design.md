# Phase 17b — Instance Registration Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 17b — 한 automation run이 생성한 role/channel/message를 **명시적 manifest로 선택**해 guild-scoped `AutomationInstance`로 등록하고, `InstanceId`를 typed runtime binding으로 남긴다. 순수 Mock(InMemory store). **참가(join)는 17c.**
- **선행**: 17a(Instance Registry Core: 모델/Store/InMemory). 16g/16h/16i(created-linking, PostPanel).

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. no_ai_gateway 유지. 17b는 **등록 side만** — run/handle_event/runner에 InstanceStore + InstanceIdGenerator 배선. **join/custom_id 라우팅/DB는 후속.**

**목표(한 문장):** `RegisterInstance` 액션이 현재 run의 created role/channel/message를 **명시적 의미 key**로 선택→guild-scoped `AutomationInstance`로 store에 등록→생성된 `InstanceId`를 action key에 바인딩.

**핵심 프레임:** RegisterInstance는 16g의 **typed output binding 패턴을 그대로** 쓰지만, InstanceStore에 **영속 가능한 side effect**를 만들기 때문에 (a)명시적 resource manifest, (b)Store 실패 처리, (c)guild-scoped metadata가 추가로 필요.

---

## 0. 범위

**포함:** `ActionSpec::RegisterInstance{key, kind, resources}` + `InstanceResourceRefs`(roles/channels/messages: `BTreeMap<String, CreatedRef>`, created-only) + `InstanceIdGenerator` trait + `SequenceInstanceIdGenerator` + `CreatedResource`에 key + **created_messages 바인딩**(PostPanel에 key) + run/handle_event/runner 배선(InstanceStore/generator) + RuntimeContext actor·ruleset_key threading + validate + Mock 테스트.

**제외(→17c/17d):** dynamic join · instance custom_id · join 핸들러 · post-join-button · **InstanceRef 소비**(17c: `{instance:{created:key}}`) · Existing/adopted 리소스 등록 · DB · Store 실패 rollback/reconciliation · bounded retry.

---

## 1. 명시적 manifest (자동수집 금지)
한 run이 임시역할·멤버역할·채널·로그메시지·welcome패널·테스트메시지를 만들 수 있음. "전부 등록"하면 **어느 게 join 역할/주 채널/welcome인지 모름**. 한 run이 instance 2개를 만들거나 일부는 instance 무관일 수도. 그래서 RegisterInstance가 **명시적으로 선택 + 의미 이름 매핑**:
```yaml
resources:
  roles:
    member_role: { created: study_member_role }
  channels:
    room_channel: { created: study_channel }
  messages:
    welcome_panel: { created: study_welcome_panel }
```
→ `resources.roles["member_role"] → RoleId`. 17c가 이 alias("member_role")로 join 역할을 찾음.

---

## 2. 모델 (automation-state)

```rust
enum ActionSpec {
    ..., DeferEphemeral, EditResponse {..},
    RegisterInstance {
        key: String,                    // output key — InstanceId 바인딩
        kind: InstanceKind,             // automation_instance::InstanceKind("study_room")
        resources: InstanceResourceRefs,
    },
}

#[serde(deny_unknown_fields)]
pub struct InstanceResourceRefs {
    #[serde(default)] pub roles: BTreeMap<String, CreatedRef>,     // alias → {created: CreateRole key}
    #[serde(default)] pub channels: BTreeMap<String, CreatedRef>,  // alias → {created: CreateChannel key}
    #[serde(default)] pub messages: BTreeMap<String, CreatedRef>,  // alias → {created: PostPanel key}
}
```
- **created-only**: 세 map 모두 `CreatedRef`(16g, `{created:key}`). map이 리소스 타입 결정(roles map → CreateRole 참조). Existing/adopted은 후속.
- `kind`는 automation-state가 `automation-instance::InstanceKind` 재노출 or 재정의 — 플랜에서 dep 방향 확정(automation-state→automation-instance).
- **문자열 템플릿 `${created.x.id}` 금지** — 타입 참조만(16g 원칙 유지).

---

## 3. CreatedResource key + created_messages + PostPanel key

- **`CreatedResource`에 key 추가**(audit/관찰성): `Role{key, ...}`/`Channel{key, ...}`/`Message{key, ...}`. run이 채움.
- **`CreatedResource::Instance{key, id: InstanceId}` 추가**(보강 #5) — RegisterInstance 성공 시 run이 RunResult에 기록. 외부 관찰자가 store 재조회 없이 등록된 instance를 앎. (이미 CreatedResource 수정하니 지금 같이.)
- **RegisterInstance의 source of truth = RuntimeBindings(typed)**, `Vec<CreatedResource>` 아님. run은 `created_roles`/`created_channels`/**`created_messages`**(신규)에서 CreatedRef 해소.
- **`PostPanel`에 `key: String` 추가** → run이 `created_messages[key] = message_id` 바인딩(16i PostPanel엔 key 없음). CreatedResource::Message에도 key.
- 정리: `CreatedResource` = 실행 결과/audit. RuntimeBindings = RegisterInstance 해소용.

---

## 4. InstanceIdGenerator (automation-instance)

```rust
pub trait InstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError>;
}
pub enum InstanceIdGenerationError { /* 최소 */ Invalid }

pub struct SequenceInstanceIdGenerator { /* prefix: String + AtomicU64 */ }
impl SequenceInstanceIdGenerator { pub fn new(prefix: &str, start: u64) -> Self; }
// generate() → InstanceId::parse(format!("{prefix}_{n:03}"))?. 범용 — prefix는 호출자가.
```
- **범용**: `new("inst", 1)` → `inst_001`. `room_001`은 StudyRoom fixture(`new("room", 1)`)에서만. 라이브러리에 하드코딩 금지.
- 17a Store가 id를 mint 안 하는 결정과 연결 — **호출자가 generator 제공**. 실제 production은 Random/Base32/ULID(후속, custom_id 길이·idempotency 결정 시).
- **충돌 fail-fast**: generate 1회 → `Store.register` → `DuplicateInstance`면 **action 실패**(재시도/랜덤재생성 없음). bounded retry는 후속.

---

## 5. metadata는 시스템이 채움 (사용자 입력 금지)
`RegisterInstance` action이 선언하는 건 **key, kind, resource refs→aliases**뿐. 나머지는 runtime context:
```rust
AutomationInstance {
    id:        generator.generate()?,       // 시스템
    guild_id:  context.guild_id,            // 시스템
    ruleset_key: context.ruleset_key,       // 시스템 (threading)
    kind:      action.kind.clone(),         // action
    created_by: context.actor,              // 시스템 (RuntimeContext에 actor 추가)
    resources: resolved,                    // action refs → RuntimeBindings 해소
    status:    InstanceStatus::Active,       // 시스템
}
```
- **RuntimeContext에 `actor: UserId` 추가**(from_event가 event.actor). **ruleset_key는 run/handle_event에 threading**(16k failure_message처럼) — 아니면 RuntimeContext에. AI/rule author가 guild/user/status 임의 지정 불가.

---

- run/handle_event가 `instances: &impl InstanceStore` + `instance_ids: &impl InstanceIdGenerator` + `ruleset_key: &str` 추가로 받음.
- **RegisterInstance arm 순서(불변식):**
  1. manifest의 모든 CreatedRef를 typed binding(created_*)으로 해소 → 실패 시 error(generator/store 호출 안 함).
  2. `InstanceResources` 완성.
  3. `instance_ids.generate()?` → 실패 시 error(store 호출 안 함).
  4. `AutomationInstance` 구성(시스템 metadata).
  5. `instances.register(instance).await?` → 실패 시 error(**binding 안 만듦**).
  6. **register 성공 후에만** `instance_bindings[action.key] = id`.
  7. `created.push(CreatedResource::Instance{key, id})`.
  - **절대 금지:** binding을 register 전에 넣기(뒤 action이 없는 instance 참조). 각 단계 실패는 fail-fast(`?`).
- runner가 gateway로부터 store/generator 전달. **`instance_bindings: BTreeMap<String, InstanceId>`**(RuntimeBindings 확장) — 17c의 `InstanceRef::Created(key)`가 소비.
- **17b = 마지막 param-threading Phase.** 17c 전에 `AutomationServices{mutation, responder, instances, instance_ids}` bundle 도입 예정(17b는 defer, param threading — 컴파일러 가이드로 호출자 다수 갱신).

---

## 7. Store 실패 = fail-fast + orphan 경계
- `instances.register` 실패(17b InMemory=DuplicateInstance; 후속 Postgres=connection/timeout/unique/serialization) → **normal action failure, fail-fast**(binding 안 만듦 → 뒤 instance-ref action 미실행 → 16k deferred fallback으로 사용자 실패 메시지).
- **명시 경계(숨기지 않음):** *Known boundary: Registration failure after Discord resource creation may leave unregistered (orphan) resources. Lifecycle/reconciliation/compensation is deferred.* (Discord 역할·채널은 이미 생성됐을 수 있음.)

---

## 8. validate (RegisterInstance)

**output symbol table 기반**(보강 #1): rule action을 순서대로 훑어 `created: key → {Role|Channel|Message|Instance}` 심볼 테이블 구성(16g/16h created map을 Message/Instance로 확장). manifest bucket을 이 테이블과 비교.

1. **DuplicateActionKey** — RegisterInstance.key가 다른 **output action key**(CreateRole/CreateChannel/PostPanel/RegisterInstance)와 중복. → **output key는 하나의 namespace**(아래).
2. **타입별 ref 검사** — roles map의 CreatedRef는 심볼 테이블에서 **CreateRole**이어야(아니면 mismatch/unknown), channels→CreateChannel, messages→**PostPanel**. 예: `roles: {member_role: {created: study_channel}}` → 실패. `messages: {welcome_panel: {created: study_member_role}}` → 실패.
3. **forward-ref 금지** — RegisterInstance는 참조하는 create/post action **뒤**에(심볼 테이블은 순서대로 누적).
4. **alias 규칙** — 각 map의 alias(map key: "member_role" 등): **1~32, `[a-zA-Z0-9_-]`, 빈 문자열 금지**. 거부: `""`, `"member role"`, `"../role"`, `"member:role"`. (`InstanceResourceKey` newtype이 이상적이나 17b는 String + validate 검사로.)
5. **빈 instance 금지** — 세 map 전부 비면 실패(Store primitive는 빈 것 허용하나 rule은 실수).
6. created-only(현재 run 생성만; Existing은 17b 미지원).

**key namespace 3분리(스펙 명시):** ① **action output key**(CreateRole/CreateChannel/PostPanel/RegisterInstance — 한 namespace, 중복 금지) · ② **button key**(PostPanel/PanelSpec 버튼 — 별도) · ③ **rule key**(별도). 혼동 금지.

새 ValidationError: `EmptyInstanceResources{rule}`, `EmptyResourceAlias{rule}`(또는 InvalidResourceAlias), `UnknownCreatedMessageRef{rule, key}`, `CreatedMessageRefTypeMismatch{rule, key}` 등(role/channel은 16g/16h 변형 재사용, message/instance 신규).

---

## 9. StudyRoom rule (17b)
```yaml
- { defer_ephemeral }
- { create_role,   key: study_member_role, name: "${input.room_name} 멤버" }
- { create_channel, key: study_channel,      name: "study-${input.room_name}" }
- { upsert_overwrite, channel: {created: study_channel}, target: everyone,                        deny:  view_channel }
- { upsert_overwrite, channel: {created: study_channel}, target: {role:{created: study_member_role}}, allow: view_channel }
- { grant_role, role: {created: study_member_role}, target: actor }
- { post_panel, key: study_welcome_panel, channel: {created: study_channel}, content: "환영합니다.", buttons: [{key: study_help, label: 도움말}] }
- { register_instance, key: study_room_instance, kind: study_room,
    resources: { roles: {member_role: {created: study_member_role}},
                 channels: {room_channel: {created: study_channel}},
                 messages: {welcome_panel: {created: study_welcome_panel}} } }
- { edit_response, content: "스터디룸 생성 완료!" }
```
17b: `study_room_instance` binding 생성 + store 등록까지. 17c가 뒤에 PostJoinPanel(instance={created: study_room_instance}) 추가.

---

## 10. 테스트 (핵심)
1. RegisterInstance가 refs 해소→AutomationInstance 등록(store.get으로 확인, guild/ruleset_key/created_by/kind/resources/status 정확).
2. resolved resources: roles["member_role"]=생성 RoleId, channels["room_channel"]=ChannelId, messages["welcome_panel"]=MessageId.
3. instance_id가 SequenceInstanceIdGenerator 값 + **RunResult에 `CreatedResource::Instance{key, id}` 기록**(보강 #5).
4. metadata 시스템값(guild_id=context, created_by=actor, status=Active) — action이 넣은 값 아님.
5. **store register 실패(Duplicate) → instance_binding 없음 + fail-fast**(뒤 action 미실행), deferred면 16k fallback.
6. validate(symbol table): register key 중복 / role alias가 CreateChannel 참조(type) / message ref가 role 참조 / missing ref / forward-ref / 빈 resources / 빈·잘못된 alias(`"member role"`).
7. created_messages: PostPanel key로 message id 바인딩 → messages ref 해소.
8. 전체 StudyRoom run(defer→...→register→edit) call 순서 + store에 instance 1개 + 기존 300 semantics 유지(additive).
9. 같은 InstanceId라도 다른 guild면 등록 가능(17a store + 17b 통합).

---

## 11. 하지 않는 것 (Forbidden — 17c/17d)
dynamic join · instance custom_id(7-seg) · join 핸들러 · post-join-button · InstanceRef 소비 · Existing/adopted 등록 · DB · Store 실패 rollback · bounded retry · services bundle 리팩터.

---

## 12. 로드맵 (조정)
```
17a✅ Instance Registry   17b▶ Instance Registration Core (이 스펙)
17c Dynamic Join Core (+ AutomationServices bundle)   17d PostgreSQL InstanceStore(persistence)   17e Durable Dynamic Join Live
```
(17d DB → 17e live: join-live의 가치는 "재시작 후에도 참가 버튼이 산다"라 persistence-before-live.)

**dependency 조건(중요):** `automation-instance`는 **순수 core crate 유지**(모델+trait+InMemory만). 17d PostgreSQL impl은 **별도 edge crate**(예: `automation-instance-postgres`)에 — automation-instance가 sqlx/DB에 의존하면 안 됨(automation-state→automation-instance→sqlx 이상 의존 방지). seam 패턴이 이걸 가능케 함.

---

## 13. Codex 핸드오프 (개요)
1. automation-instance: InstanceIdGenerator trait + InstanceIdGenerationError + SequenceInstanceIdGenerator + 재노출.
2. automation-state: ActionSpec::RegisterInstance + InstanceResourceRefs + InstanceKind 사용(dep automation-instance) + serde 테스트.
3. automation-core: dep automation-instance + CreatedResource key + PostPanel key + created_messages 바인딩 + PlannedAction::RegisterInstance + interpret arm(created-only pass-through) + run arm(해소·등록·binding) + RuntimeContext actor + run/handle_event에 instances/instance_ids/ruleset_key + validate + 호출자 갱신 + 테스트.
4. automation-runtime: gateway/runner에 InstanceStore + InstanceIdGenerator 배선(tool이 InMemoryInstanceStore + SequenceInstanceIdGenerator 제공).
5. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push. **live 없음**(17e).

## 최종 정리
17b = Instance Registration Core. `RegisterInstance{key, kind, resources: created-only alias→CreatedRef maps}`가 현재 run 생성 리소스를 명시적으로 선택→RuntimeBindings(created_roles/channels/messages)로 해소→guild-scoped metadata(시스템)로 AutomationInstance 구성→InstanceIdGenerator로 id→InstanceStore.register(실패=fail-fast, orphan 경계 명시)→InstanceId를 action key에 바인딩. 16g typed-binding 패턴 + 영속 side effect(manifest/실패처리/metadata). 17c가 InstanceRef로 join.
