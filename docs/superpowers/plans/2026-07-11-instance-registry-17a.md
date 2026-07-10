# Phase 17a — Automation Instance Registry Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** 동적 실행 결과(role/channel/message)를 generic `AutomationInstance`로 묶어 `InstanceStore` trait(InMemory)로 (guild_id, instance_id) 저장·조회·나열·상태변경하는 순수 registry primitive.

**Architecture:** 새 크레이트 `automation-instance`(automation-state와 병렬). 모델(InstanceId 검증 newtype, InstanceKind, InstanceResources map, InstanceStatus, AutomationInstance) + InstanceStore trait + InMemoryInstanceStore(guild-scoped nested BTreeMap). **run 배선/DB/dynamic join 없음.**

## Global Constraints
- **코드 주석 금지.** **Codex 구현.**
- **Store는 id mint 안 함**(호출자 제공). InstanceId는 Deserialize에서 검증. (guild_id, instance_id) composite key.
- no_ai_gateway 가드. 완료 게이트: build(경고0)/test/clippy(`--all-targets -- -D warnings`)/fmt. 완료 후 push.

---

## Task 1: automation-instance 크레이트 + 모델 + Store

- [ ] **Step 1: `Cargo.toml`(신규) + workspace member**

`crates/automation-instance/Cargo.toml`:
```toml
[package]
name = "automation-instance"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
discord-model = { path = "../discord-model" }

[dev-dependencies]
serde_json = { workspace = true }
futures = { workspace = true }
```
> `futures`가 workspace dep 아니면 `futures = "0.3"`. (automation-core dev-dep 형태 대조.)

workspace `Cargo.toml`의 members에 `"crates/automation-runtime"` 다음 추가:
```toml
    "crates/automation-instance",
```

- [ ] **Step 2: `src/id.rs` — InstanceId(검증 + 전체 trait)**

```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceIdError {
    Empty,
    TooLong,
    InvalidChar,
}

impl InstanceId {
    pub fn parse(value: &str) -> Result<Self, InstanceIdError> {
        if value.is_empty() {
            return Err(InstanceIdError::Empty);
        }
        if value.len() > MAX_LEN {
            return Err(InstanceIdError::TooLong);
        }
        if !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
        {
            return Err(InstanceIdError::InvalidChar);
        }
        Ok(InstanceId(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for InstanceId {
    type Err = InstanceIdError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        InstanceId::parse(value)
    }
}

impl TryFrom<String> for InstanceId {
    type Error = InstanceIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        InstanceId::parse(&value)
    }
}

impl AsRef<str> for InstanceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for InstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for InstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        InstanceId::parse(&value).map_err(|error| serde::de::Error::custom(format!("{error:?}")))
    }
}
```

- [ ] **Step 3: `src/model.rs` — 모델**

```rust
use std::collections::BTreeMap;

use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};
use serde::{Deserialize, Serialize};

use crate::id::InstanceId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceKind(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceResources {
    #[serde(default)]
    pub roles: BTreeMap<String, RoleId>,
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelId>,
    #[serde(default)]
    pub messages: BTreeMap<String, MessageId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Active,
    Disabled,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

- [ ] **Step 4: `src/store.rs` — trait + InMemory + Error**

```rust
use std::collections::BTreeMap;
use std::sync::Mutex;

use discord_model::GuildId;

use crate::id::InstanceId;
use crate::model::{AutomationInstance, InstanceStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceStoreError {
    DuplicateInstance,
    NotFound,
}

#[allow(async_fn_in_trait)]
pub trait InstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError>;
    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError>;
    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError>;
    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError>;
}

#[derive(Default)]
pub struct InMemoryInstanceStore {
    inner: Mutex<BTreeMap<GuildId, BTreeMap<InstanceId, AutomationInstance>>>,
}

impl InMemoryInstanceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InstanceStore for InMemoryInstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let entries = guilds.entry(instance.guild_id).or_default();
        if entries.contains_key(&instance.id) {
            return Err(InstanceStoreError::DuplicateInstance);
        }
        entries.insert(instance.id.clone(), instance);
        Ok(())
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&guild_id)
            .and_then(|entries| entries.get(instance_id))
            .cloned())
    }

    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&guild_id)
            .map(|entries| entries.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let instance = guilds
            .get_mut(&guild_id)
            .and_then(|entries| entries.get_mut(instance_id))
            .ok_or(InstanceStoreError::NotFound)?;
        instance.status = status;
        Ok(())
    }
}
```

> **clippy 주의:** InMemory의 async fn들은 내부에 `.await`가 없음(즉시 반환, Postgres 대비 시그니처). clippy `unused_async`는 **trait impl 메서드엔 발동 안 함**(시그니처가 trait에 고정). 혹시 발동하면 impl 블록에 `#[allow(clippy::unused_async)]`. Mutex guard는 await를 안 넘으므로 `await_holding_lock` 무관.

- [ ] **Step 5: `src/lib.rs` — 재노출**

```rust
pub mod id;
pub mod model;
pub mod store;

pub use id::{InstanceId, InstanceIdError};
pub use model::{AutomationInstance, InstanceKind, InstanceResources, InstanceStatus};
pub use store::{InMemoryInstanceStore, InstanceStore, InstanceStoreError};
```

- [ ] **Step 6: 빌드 + 커밋**

Run: `cargo build -p automation-instance` (경고 0)
```bash
git add crates/automation-instance Cargo.toml
git commit -m "feat(automation-instance): instance registry model + store"
```

---

## Task 2: 테스트 + 게이트 + push

- [ ] **Step 1: `tests/no_ai_gateway.rs`**

```rust
#[test]
fn manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
```

- [ ] **Step 2: `tests/registry.rs`**

```rust
use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceIdError, InstanceKind,
    InstanceResources, InstanceStatus, InstanceStore, InstanceStoreError,
};
use discord_model::{GuildId, RoleId, UserId};
use futures::executor::block_on;

fn instance(guild: u64, id: &str) -> AutomationInstance {
    let mut roles = BTreeMap::new();
    roles.insert("member_role".to_string(), RoleId(100));
    AutomationInstance {
        id: InstanceId::parse(id).unwrap(),
        guild_id: GuildId(guild),
        ruleset_key: "studyroom_demo".to_string(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(3),
        resources: InstanceResources {
            roles,
            channels: BTreeMap::new(),
            messages: BTreeMap::new(),
        },
        status: InstanceStatus::Active,
    }
}

#[test]
fn instance_id_parse_validation() {
    assert!(InstanceId::parse("study_room_1").is_ok());
    assert!(InstanceId::parse("room-1").is_ok());
    assert_eq!(InstanceId::parse("").unwrap_err(), InstanceIdError::Empty);
    assert_eq!(
        InstanceId::parse(&"a".repeat(33)).unwrap_err(),
        InstanceIdError::TooLong
    );
    assert_eq!(
        InstanceId::parse("room 1").unwrap_err(),
        InstanceIdError::InvalidChar
    );
    assert_eq!(
        InstanceId::parse("room/1").unwrap_err(),
        InstanceIdError::InvalidChar
    );
    assert_eq!(
        InstanceId::parse("한글방").unwrap_err(),
        InstanceIdError::InvalidChar
    );
}

#[test]
fn bad_instance_id_json_rejected() {
    assert!(serde_json::from_str::<InstanceId>(r#""room 1""#).is_err());
    assert!(serde_json::from_str::<InstanceId>(r#""room:1""#).is_err());
    assert!(serde_json::from_str::<InstanceId>(r#""study_room_1""#).is_ok());
}

#[test]
fn automation_instance_serde_roundtrip() {
    let value = instance(7, "study_room_1");
    let json = serde_json::to_string(&value).unwrap();
    let back: AutomationInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(value, back);
}

#[test]
fn register_then_get() {
    let store = InMemoryInstanceStore::new();
    let value = instance(7, "room1");
    block_on(store.register(value.clone())).unwrap();
    assert_eq!(
        block_on(store.get(GuildId(7), &InstanceId::parse("room1").unwrap())).unwrap(),
        Some(value)
    );
}

#[test]
fn duplicate_register_fails() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(7, "room1"))).unwrap();
    assert_eq!(
        block_on(store.register(instance(7, "room1"))).unwrap_err(),
        InstanceStoreError::DuplicateInstance
    );
}

#[test]
fn same_id_allowed_across_guilds() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(1, "room1"))).unwrap();
    block_on(store.register(instance(2, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    assert_eq!(
        block_on(store.get(GuildId(1), &id)).unwrap().unwrap().guild_id,
        GuildId(1)
    );
    assert_eq!(
        block_on(store.get(GuildId(2), &id)).unwrap().unwrap().guild_id,
        GuildId(2)
    );
}

#[test]
fn guild_isolation() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(1, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    assert!(block_on(store.get(GuildId(2), &id)).unwrap().is_none());
    assert!(block_on(store.list_by_guild(GuildId(2))).unwrap().is_empty());
    assert_eq!(block_on(store.list_by_guild(GuildId(1))).unwrap().len(), 1);
}

#[test]
fn list_by_guild_deterministic() {
    let store = InMemoryInstanceStore::new();
    for id in ["c", "a", "b"] {
        block_on(store.register(instance(7, id))).unwrap();
    }
    let ids: Vec<String> = block_on(store.list_by_guild(GuildId(7)))
        .unwrap()
        .into_iter()
        .map(|value| value.id.as_str().to_string())
        .collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

#[test]
fn update_status_works() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(7, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    block_on(store.update_status(GuildId(7), &id, InstanceStatus::Disabled)).unwrap();
    assert_eq!(
        block_on(store.get(GuildId(7), &id)).unwrap().unwrap().status,
        InstanceStatus::Disabled
    );
}

#[test]
fn update_status_missing_not_found() {
    let store = InMemoryInstanceStore::new();
    let id = InstanceId::parse("ghost").unwrap();
    assert_eq!(
        block_on(store.update_status(GuildId(7), &id, InstanceStatus::Disabled)).unwrap_err(),
        InstanceStoreError::NotFound
    );
}

#[test]
fn returned_clone_does_not_mutate_store() {
    let store = InMemoryInstanceStore::new();
    block_on(store.register(instance(7, "room1"))).unwrap();
    let id = InstanceId::parse("room1").unwrap();
    let mut fetched = block_on(store.get(GuildId(7), &id)).unwrap().unwrap();
    fetched.status = InstanceStatus::Deleted;
    fetched.resources.roles.clear();
    let again = block_on(store.get(GuildId(7), &id)).unwrap().unwrap();
    assert_eq!(again.status, InstanceStatus::Active);
    assert_eq!(again.resources.roles.len(), 1);
}
```

- [ ] **Step 3~6: 게이트 + push**
- `cargo build` (경고 0) / `cargo test` (전체 ~312; 300 + registry 11 + no_ai_gateway 1) / `cargo clippy --all-targets -- -D warnings` (0) / `cargo fmt --all -- --check` / `git push origin main`.
- 커밋: `feat(automation-instance): registry tests`

---

## Self-Review (스펙 대비)
- 새 크레이트 automation-instance(모델+trait+in-memory+no_ai_gateway) + workspace member ✅.
- InstanceId: parse(1~32, `[a-zA-Z0-9_-]`) + Display/FromStr/TryFrom/AsRef + custom Serialize(string)/Deserialize(검증) + Ord/Hash ✅.
- generic 모델(InstanceKind(String), InstanceResources map, Active/Disabled/Deleted), deny_unknown_fields ✅.
- InstanceStore trait(register/get/list_by_guild/update_status) + InMemory(guild-scoped nested BTreeMap, Store id mint 안 함) + InstanceStoreError(Duplicate/NotFound) ✅.
- guild isolation(같은 id 다른 guild 허용, get/list 격리) + list 결정론 + clone 독립 + JSON invalid id 거부 ✅.
- clippy: or_default/unwrap_or_default/and_then, async fn in trait allow, 주석 없음 ✅.

## Codex 핸드오프 (권장 2청크)
- **청크 A** = Task 1(크레이트 + 모델 + Store). build. 커밋 1개.
- **청크 B** = Task 2(테스트 + 게이트 + push). 전체 build/test/clippy/fmt + push. 커밋 1개 + push.
보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + 이탈.
