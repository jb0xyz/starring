# Phase 17a — Automation Instance Registry Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 17a — 동적 자동화 실행으로 만들어진 role/channel/message를 하나의 `AutomationInstance`로 묶어 저장·조회하는 **순수 registry primitive**. in-memory + Mock. run 배선/DB/dynamic join 없음.
- **선행**: 16a~16k(Layer 2 코어 + live). 17b(dynamic join)/17d(DB)의 기반.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. `no_ai_gateway` 가드 유지. **17a는 순수 primitive** — 새 크레이트 `automation-instance`만(모델 + Store trait + in-memory). **handle_event/run 무배선.**

**목표(한 문장):** 동적 실행으로 만든 role/channel/message를 `AutomationInstance`로 묶어 (guild_id, instance_id)로 저장·조회·나열·상태변경하는 registry layer.

---

## 0. 범위

**포함:** `AutomationInstance` 모델 + `InstanceId`(검증) + `InstanceKind(String)` + `InstanceResources`(map) + `InstanceStatus` + `InstanceStore` **trait** + `InMemoryInstanceStore` impl + `InstanceStoreError` + Mock 테스트 + no_ai_gateway 가드.

**제외(→17b/17d):** handle_event 배선 · CreatedResource 자동 등록 · **instance_id 생성**(Store는 mint 안 함, 호출자 제공) · dynamic join · custom_id instance 라우팅 · DB · Discord resource cleanup/delete · lifecycle policy · audit persistence · Failed 상태.

---

## 1. 왜 registry-first인가 (dynamic join 앞에)
dynamic join = "버튼 하나" 같지만 실은 **상태 관리**: join 클릭 → 어느 instance인지 → 그 role_id 조회 → 지급. custom_id에 role_id/channel_id를 박으면 100자 제한·재시작 소실·DB 이관 시 포맷 교체·보안검증 어려움. **`instance:<instance_id>` 인디렉션 + registry 조회**가 유일하게 안 꼬임. 그래서 registry를 먼저 세운다. (지금 `run()`이 반환하는 `Vec<CreatedResource>`가 이미 "한 run이 만든 것"이라 17b가 그걸 캡처만 하면 instance가 됨.)

---

## 2. 모델 (automation-instance)

```rust
pub struct InstanceId(String);   // parse로만 생성, custom serde 검증

impl InstanceId {
    pub fn parse(value: &str) -> Result<Self, InstanceIdError>;   // 1~32, ASCII [a-zA-Z0-9_-], 공백/기타 금지
    pub fn as_str(&self) -> &str;
}
pub enum InstanceIdError { Empty, TooLong, InvalidChar }
// InstanceId trait: Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash + Display + FromStr(=parse) + TryFrom<String> + AsRef<str> + Serialize(inner string) + Deserialize(parse 검증). 거부 예: "", "room 1", "room/1", "room:1", "../x", "한글방", 33자+.

pub struct InstanceKind(String);   // "study_room" 등. enum 승격은 후속.

pub struct InstanceResources {     // 고정 필드 아닌 map(일반 자동화 대응)
    pub roles: BTreeMap<String, RoleId>,       // "member_role" → RoleId
    pub channels: BTreeMap<String, ChannelId>, // "room_channel" → ChannelId
    pub messages: BTreeMap<String, MessageId>, // "welcome_panel" → MessageId
}

pub enum InstanceStatus { Active, Disabled, Deleted }   // snake_case serde, 최소

pub struct AutomationInstance {
    pub id: InstanceId,
    pub guild_id: GuildId,
    pub ruleset_key: String,
    pub kind: InstanceKind,
    pub created_by: UserId,
    pub resources: InstanceResources,
    pub status: InstanceStatus,
}
```
- 전부 `Serialize/Deserialize`(deny_unknown_fields) + Clone/Debug/PartialEq/Eq. `InstanceId`는 custom serde(직렬화=string, 역직렬화=`parse` 검증 — 잘못된 id JSON 거부). `InstanceId`/`GuildId`는 Ord(BTreeMap key).
- **StudyRoom 전용 아님** — kind 값 하나. role/channel/message는 여러 개 가능(map).
- `ruleset_key: String` 유지(17a). **TODO(후속):** `RuleSetKey(String)` newtype 검증. InstanceId는 dynamic custom_id 라우팅에 들어가서 지금 검증; ruleset_key 검증은 defer.

---

## 3. Store trait + InMemory + Error

```rust
pub enum InstanceStoreError { DuplicateInstance, NotFound }

#[allow(async_fn_in_trait)]
pub trait InstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError>;
    async fn get(&self, guild_id: GuildId, instance_id: &InstanceId)
        -> Result<Option<AutomationInstance>, InstanceStoreError>;
    async fn list_by_guild(&self, guild_id: GuildId)
        -> Result<Vec<AutomationInstance>, InstanceStoreError>;
    async fn update_status(&self, guild_id: GuildId, instance_id: &InstanceId, status: InstanceStatus)
        -> Result<(), InstanceStoreError>;
}
```
- **Store는 id를 mint하지 않음** — 호출자가 `AutomationInstance`(id 포함) 넘김. id 생성기는 17b.
- `register`: **`instance.guild_id` 기준 저장**, 같은 guild에 같은 `instance.id` 있으면 `DuplicateInstance`, **다른 guild의 같은 id는 허용**. `get`: 없으면 `Ok(None)`(에러 아님). `list_by_guild`: 그 guild의 instance를 InstanceId 정렬로. `update_status`: 없으면 `NotFound`.
- **빈 resources 허용** — *Phase 17a does not require resources to be non-empty. Resource completeness is validated by higher-level workflows.* (partial/initial registration 대비.)
- `InstanceStoreError::Backend(String)`은 **17d(Postgres)에서 추가** — 17a는 DuplicateInstance/NotFound만.
- **InMemoryInstanceStore**: `Mutex<BTreeMap<GuildId, BTreeMap<InstanceId, AutomationInstance>>>`. guild isolation + 결정론적 list. async fn 내부 await 없음(즉시 반환; 미래 Postgres용 시그니처). native async fn in trait(static dispatch), 테스트는 `block_on`.
- **소유권**: get/list는 **clone 반환**(반환본 수정이 store 내부에 영향 없음).

---

## 4. Guild isolation
(guild_id, instance_id) composite key. guild A의 instance는 guild B의 get/list에서 안 보임. 멀티테넌트 기본 안전장치. 같은 instance_id가 다른 guild에 공존 가능.

---

## 5. 크레이트
새 `crates/automation-instance`: `[dependencies]` discord-model, serde. `[dev-dependencies]` serde_json, futures. `tests/no_ai_gateway.rs`(자기 Cargo.toml에 ai-gateway 문자열 차단). lib.rs가 모델/trait/impl/error 재노출.

---

## 6. 테스트 (11)
1. InstanceId parse 통과("study_room_1") / 실패(빈문자열·33자·공백·`!` 등).
2. AutomationInstance serde roundtrip.
3. InstanceResources roles/channels/messages 저장 roundtrip.
4. InMemory register → get(Some, 값 일치).
5. 중복 (guild,id) register → DuplicateInstance.
6. **guild isolation**: guild A에 register한 걸 guild B get→None, list→빈 벡터.
7. list_by_guild 결정론(InstanceId 정렬).
8. update_status Active → Disabled 후 get으로 확인.
9. 없는 instance update_status → NotFound.
10. get 반환본 clone 수정 → store 내부 불변.
11. InstanceId JSON 역직렬화 검증(잘못된 id 문자열 → 실패). (+ no_ai_gateway 가드.)

---

## 7. 하지 않는 것 (Forbidden — 17b/17d)
handle_event/run 배선 · CreatedResource 자동 등록 · instance_id 생성 · dynamic join · custom_id instance 라우팅 · DB · Discord cleanup/delete · Failed 상태 · audit.

---

## 8. 17b 예고 (배선)
```
StudyRoom run 성공 → CreatedResource 수집 → InstanceResources 매핑 → InstanceId 생성(InstanceIdGenerator)
→ InstanceStore.register(instance) → hub join 버튼 custom_id에 instance_id 포함
join 클릭 → custom_id에서 instance_id → InstanceStore.get(guild, id) → resources.roles["member_role"] → GrantRole(actor)
```

---

## 9. 로드맵
```
16k✅ Deferred lifecycle   17a▶ Instance Registry Core (이 스펙)
17b Dynamic Join Core   17c Dynamic Join Live   17d DB Persistence   17e Backend API / Web UI
```

---

## 10. Codex 핸드오프 (개요)
1. 새 crate automation-instance: 모델(InstanceId+검증/custom serde, InstanceKind, InstanceResources, InstanceStatus, AutomationInstance) + InstanceStoreError + InstanceStore trait + InMemoryInstanceStore + lib 재노출 + Cargo.toml(workspace member 추가).
2. tests/registry.rs(11) + tests/no_ai_gateway.rs.
3. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push.

## 최종 정리
17a = Automation Instance Registry Core. 동적 실행 결과(role/channel/message)를 generic `AutomationInstance`(kind=String, resources=map)로 묶어 `InstanceStore` trait(InMemory impl)로 (guild_id, instance_id) 저장·조회·나열·상태변경. **Store는 id mint 안 함**(호출자 제공, 생성기는 17b). InstanceId는 custom_id-safe 검증. 순수 primitive — run 배선/DB/join은 후속. seam 패턴이라 17d Postgres가 drop-in.
