# Desired State Schema Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`). **이 프로젝트는 Codex가 구현한다.** 각 Task는 독립 테스트 가능한 산출물로 끝난다.

**Goal:** `crates/desired-state` — 선언형 목표 상태 스키마 타입 + `validate()` 검증(6규칙)을 TDD로 구축한다.

**Architecture:** 별도 타입 스키마(`discord-model`의 primitive만 재사용). 모드 기반 선언 의미론(patch/scoped/full), 정체성(key/match/ownership/state), 고수준 access intent + raw escape hatch. 순수 데이터 + 검증만(Compiler/binding/Diff는 범위 밖).

**Tech Stack:** Rust edition 2021 stable, `serde`, `serde_json`(dev), `thiserror`, `discord-model`(path dep).

## Global Constraints

> ⚠️ **주석 금지(전역)**: 코드에 `//`, `///`, `//!` 없음. 아래 코드 블록에 주석이 있으면 제거하고 구현.

- Rust edition 2021, stable. 의존 방향 `desired-state → discord-model`(단방향). `domain`에 의존 금지.
- 파생: `Clone, Debug, PartialEq, Eq, Serialize, Deserialize` (+ 적절히 `Copy, Hash, Default, Ord`).
- ID·`Permissions`는 JSON 문자열(discord-model 타입 그대로). `ResourceKey`(String wrapper)는 맵 키로 문자열 직렬화.
- DB/sqlx 의존 없음(serde만). 검증 에러는 `thiserror`.
- 완료 게이트: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check` 전부 통과.
- 각 Task 후 멈추고 보고. `cargo add`가 아니라 워크스페이스 `[workspace.dependencies]` 참조 방식 사용.

---

### Task 1: crate 스캐폴드

**Files:**
- Modify: `Cargo.toml` (workspace members + thiserror 추가)
- Create: `crates/desired-state/Cargo.toml`, `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: `discord-model`
- Produces: 컴파일되는 `desired-state` crate(내용 비어 있음).

- [ ] **Step 1: 워크스페이스에 등록**

Modify root `Cargo.toml`: `members`에 `"crates/desired-state"` 추가, `[workspace.dependencies]`에 `thiserror = "1"` 추가. 결과:
```toml
[workspace]
members = ["crates/discord-model", "crates/domain", "crates/desired-state"]
resolver = "2"

[workspace.package]
edition = "2021"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bitflags = "2"
thiserror = "1"
```

- [ ] **Step 2: crate 파일 생성**

Create `crates/desired-state/Cargo.toml`:
```toml
[package]
name = "desired-state"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/desired-state/src/lib.rs`:
```rust
```
(빈 파일. 주석 금지 정책상 doc 주석도 없음.)

- [ ] **Step 3: 컴파일 확인**

Run: `cargo build && cargo test`
Expected: 성공, desired-state 0 tests.

- [ ] **Step 4: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "chore(desired-state): scaffold crate"
```

---

### Task 2: 정체성 primitive

**Files:**
- Create: `crates/desired-state/src/identity.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Produces: `ResourceKey(String)`, `MatchStrategy`(ByName 기본/ByExplicitId), `Ownership`(Managed 기본/Adopted/Referenced), `ResourceState`(Present 기본/Absent), `Identity{key, match_by, ownership, state}`.

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/identity.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_key_serializes_as_string() {
        let k = ResourceKey("verified_member".to_string());
        assert_eq!(serde_json::to_string(&k).unwrap(), r#""verified_member""#);
    }

    #[test]
    fn enum_defaults() {
        assert_eq!(MatchStrategy::default(), MatchStrategy::ByName);
        assert_eq!(Ownership::default(), Ownership::Managed);
        assert_eq!(ResourceState::default(), ResourceState::Present);
    }

    #[test]
    fn identity_defaults_when_absent() {
        let json = r#"{"key":"r1"}"#;
        let id: Identity = serde_json::from_str(json).unwrap();
        assert_eq!(id.key, ResourceKey("r1".to_string()));
        assert_eq!(id.match_by, MatchStrategy::ByName);
        assert_eq!(id.ownership, Ownership::Managed);
        assert_eq!(id.state, ResourceState::Present);
    }

    #[test]
    fn match_explicit_id_serde() {
        let m = MatchStrategy::ByExplicitId("123".to_string());
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"by":"by_explicit_id","value":"123"}"#);
        assert_eq!(serde_json::from_str::<MatchStrategy>(&json).unwrap(), m);
    }
}
```

Modify `crates/desired-state/src/lib.rs`:
```rust
pub mod identity;

pub use identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p desired-state`
Expected: FAIL (미정의 컴파일 에러).

- [ ] **Step 3: 구현**

`crates/desired-state/src/identity.rs` 테스트 모듈 위에 추가:
```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceKey(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MatchStrategy {
    #[default]
    ByName,
    ByExplicitId(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    #[default]
    Managed,
    Adopted,
    Referenced,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub key: ResourceKey,
    #[serde(rename = "match", default)]
    pub match_by: MatchStrategy,
    #[serde(default)]
    pub ownership: Ownership,
    #[serde(default)]
    pub state: ResourceState,
}
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test -p desired-state`
Expected: PASS (4 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add identity primitives"
```

---

### Task 3: mode + scope

**Files:**
- Create: `crates/desired-state/src/mode.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: `ResourceKey`
- Produces: `DesiredStateMode`(Patch 기본/ScopedAuthoritative/FullAuthoritative), `Scope{roles?, channels?}`, `ResourceScope`(All/Keys/NamePrefix).

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/mode.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_default_and_serde() {
        assert_eq!(DesiredStateMode::default(), DesiredStateMode::Patch);
        assert_eq!(
            serde_json::to_string(&DesiredStateMode::ScopedAuthoritative).unwrap(),
            r#""scoped_authoritative""#
        );
    }

    #[test]
    fn resource_scope_serde() {
        let s = ResourceScope::Keys(vec![ResourceKey("a".to_string())]);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"keys","value":["a"]}"#);
        assert_eq!(serde_json::from_str::<ResourceScope>(&json).unwrap(), s);
    }

    #[test]
    fn scope_roundtrip() {
        let scope = Scope { roles: Some(ResourceScope::All), channels: None };
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(serde_json::from_str::<Scope>(&json).unwrap(), scope);
    }
}
```

Modify `lib.rs` add:
```rust
pub mod mode;
```
and extend re-export line:
```rust
pub use mode::{DesiredStateMode, ResourceScope, Scope};
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p desired-state` → FAIL.

- [ ] **Step 3: 구현**

`mode.rs` 테스트 위에 추가:
```rust
use serde::{Deserialize, Serialize};

use crate::identity::ResourceKey;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredStateMode {
    #[default]
    Patch,
    ScopedAuthoritative,
    FullAuthoritative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ResourceScope {
    All,
    Keys(Vec<ResourceKey>),
    NamePrefix(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<ResourceScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<ResourceScope>,
}
```

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p desired-state` → PASS (7 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add mode and scope"
```

---

### Task 4: RoleIntent (flatten 검증)

**Files:**
- Create: `crates/desired-state/src/role.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: `Identity`, `discord_model::Permissions`
- Produces: `RoleIntent { identity(flatten), name?, permissions? }`.

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/role.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::Permissions;

    #[test]
    fn role_intent_flatten_roundtrip() {
        let r = RoleIntent {
            identity: Identity {
                key: ResourceKey("verified_member".to_string()),
                match_by: MatchStrategy::ByName,
                ownership: Ownership::Managed,
                state: ResourceState::Present,
            },
            name: Some("인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<RoleIntent>(&json).unwrap(), r);
    }

    #[test]
    fn role_intent_flatten_defaults() {
        let json = r#"{"key":"r1","name":"x"}"#;
        let r: RoleIntent = serde_json::from_str(json).unwrap();
        assert_eq!(r.identity.key, ResourceKey("r1".to_string()));
        assert_eq!(r.identity.ownership, Ownership::Managed);
        assert!(r.permissions.is_none());
    }
}
```

Modify `lib.rs`: `pub mod role;` + `pub use role::RoleIntent;`.

- [ ] **Step 2: 실패 확인** — `cargo test -p desired-state` → FAIL.

- [ ] **Step 3: 구현**

`role.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use discord_model::Permissions;

use crate::identity::Identity;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleIntent {
    #[serde(flatten)]
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
}
```

> flatten + Identity의 per-field default가 `role_intent_flatten_defaults`에서 검증된다. 만약 flatten이 default를 적용 못 해 이 테스트가 실패하면, **즉흥 우회 말고 보고**하라 — Identity 필드를 RoleIntent에 직접 넣는 폴백으로 전환한다.

- [ ] **Step 4: 통과 확인** — `cargo test -p desired-state` → PASS (9 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add RoleIntent"
```

---

### Task 5: 채널 access 타입 (고수준 + raw escape)

**Files:**
- Create: `crates/desired-state/src/access.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: `ResourceKey`, `discord_model::Permissions`
- Produces: `Capability`(View/Send/React/ManageMessages/Connect/Speak), `AccessGrant{allow, deny}`, `AccessIntent{everyone?, roles}`, `OverwriteOp`(Add 기본/Remove/Replace), `OverwriteTargetIntent`(Role(key)/Member(id)), `PermissionOverwriteIntent{target, op, allow, deny}`.

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/access.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::ResourceKey;
    use discord_model::Permissions;

    #[test]
    fn capability_serde() {
        assert_eq!(serde_json::to_string(&Capability::View).unwrap(), r#""view""#);
        assert_eq!(
            serde_json::to_string(&Capability::ManageMessages).unwrap(),
            r#""manage_messages""#
        );
    }

    #[test]
    fn access_intent_roundtrip() {
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(
            ResourceKey("verified_member".to_string()),
            AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
        );
        let a = AccessIntent {
            everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
            roles,
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<AccessIntent>(&json).unwrap(), a);
    }

    #[test]
    fn overwrite_intent_roundtrip() {
        let o = PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified_member".to_string())),
            op: OverwriteOp::Add,
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<PermissionOverwriteIntent>(&json).unwrap(), o);
        assert_eq!(OverwriteOp::default(), OverwriteOp::Add);
    }

    #[test]
    fn overwrite_target_serde() {
        let t = OverwriteTargetIntent::Member("42".to_string());
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"target":"member","id":"42"}"#);
        assert_eq!(serde_json::from_str::<OverwriteTargetIntent>(&json).unwrap(), t);
    }
}
```

Modify `lib.rs`: `pub mod access;` + `pub use access::{AccessGrant, AccessIntent, Capability, OverwriteOp, OverwriteTargetIntent, PermissionOverwriteIntent};`.

- [ ] **Step 2: 실패 확인** — `cargo test -p desired-state` → FAIL.

- [ ] **Step 3: 구현**

`access.rs` 테스트 위에:
```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use discord_model::Permissions;

use crate::identity::ResourceKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Capability {
    View,
    Send,
    React,
    ManageMessages,
    Connect,
    Speak,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessGrant {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<Capability>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessIntent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub everyone: Option<AccessGrant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<ResourceKey, AccessGrant>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwriteOp {
    #[default]
    Add,
    Remove,
    Replace,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", content = "id", rename_all = "snake_case")]
pub enum OverwriteTargetIntent {
    Role(ResourceKey),
    Member(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverwriteIntent {
    pub target: OverwriteTargetIntent,
    #[serde(default)]
    pub op: OverwriteOp,
    #[serde(default = "Permissions::empty")]
    pub allow: Permissions,
    #[serde(default = "Permissions::empty")]
    pub deny: Permissions,
}
```

- [ ] **Step 4: 통과 확인** — `cargo test -p desired-state` → PASS (13 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add channel access and raw overwrite types"
```

---

### Task 6: ChannelIntent

**Files:**
- Create: `crates/desired-state/src/channel.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: `Identity`, `AccessIntent`, `PermissionOverwriteIntent`, `ResourceKey`, `discord_model::ChannelType`
- Produces: `ChannelIntent { identity(flatten), name?, channel_type?, parent?, access?, raw_overwrites? }`.

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/channel.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{AccessGrant, AccessIntent, Capability};
    use crate::identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::ChannelType;

    #[test]
    fn channel_intent_roundtrip() {
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(
            ResourceKey("verified_member".to_string()),
            AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
        );
        let c = ChannelIntent {
            identity: Identity {
                key: ResourceKey("general_channel".to_string()),
                match_by: MatchStrategy::ByName,
                ownership: Ownership::Managed,
                state: ResourceState::Present,
            },
            name: Some("일반".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent { everyone: None, roles }),
            raw_overwrites: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<ChannelIntent>(&json).unwrap(), c);
    }
}
```

Modify `lib.rs`: `pub mod channel;` + `pub use channel::ChannelIntent;`.

- [ ] **Step 2: 실패 확인** — FAIL.

- [ ] **Step 3: 구현**

`channel.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use discord_model::ChannelType;

use crate::access::{AccessIntent, PermissionOverwriteIntent};
use crate::identity::{Identity, ResourceKey};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelIntent {
    #[serde(flatten)]
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_type: Option<ChannelType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ResourceKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_overwrites: Option<Vec<PermissionOverwriteIntent>>,
}
```

- [ ] **Step 4: 통과 확인** — PASS (14 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add ChannelIntent"
```

---

### Task 7: FeatureIntent

**Files:**
- Create: `crates/desired-state/src/feature.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: `Identity`, `ResourceKey`
- Produces: `VerificationIntent { identity(flatten), channel, grants_role }`, `ModerationIntent{}`, `LoggingIntent{}`, `FeatureIntent`(adjacent tag `feature`/`spec`).

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/feature.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};

    #[test]
    fn verification_feature_roundtrip() {
        let f = FeatureIntent::Verification(VerificationIntent {
            identity: Identity {
                key: ResourceKey("verify_panel".to_string()),
                match_by: MatchStrategy::ByName,
                ownership: Ownership::Managed,
                state: ResourceState::Present,
            },
            channel: ResourceKey("verification_channel".to_string()),
            grants_role: ResourceKey("verified_member".to_string()),
        });
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<FeatureIntent>(&json).unwrap(), f);
    }

    #[test]
    fn skeleton_feature_roundtrip() {
        let f = FeatureIntent::Moderation(ModerationIntent::default());
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<FeatureIntent>(&json).unwrap(), f);
    }
}
```

Modify `lib.rs`: `pub mod feature;` + `pub use feature::{FeatureIntent, LoggingIntent, ModerationIntent, VerificationIntent};`.

- [ ] **Step 2: 실패 확인** — FAIL.

- [ ] **Step 3: 구현**

`feature.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use crate::identity::{Identity, ResourceKey};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationIntent {
    #[serde(flatten)]
    pub identity: Identity,
    pub channel: ResourceKey,
    pub grants_role: ResourceKey,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationIntent {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingIntent {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "feature", content = "spec", rename_all = "snake_case")]
pub enum FeatureIntent {
    Verification(VerificationIntent),
    Moderation(ModerationIntent),
    Logging(LoggingIntent),
}
```

> FeatureIntent는 **인접 태깅**(`feature`/`spec`)이다. VerificationIntent가 내부에서 flatten을 쓰기 때문에, 내부 태깅으로 하면 flatten과 충돌한다. `spec` 안에 standalone 객체로 두면 flatten이 정상 동작한다.

- [ ] **Step 4: 통과 확인** — PASS (16 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add FeatureIntent"
```

---

### Task 8: DesiredState root

**Files:**
- Create: `crates/desired-state/src/state.rs`
- Modify: `crates/desired-state/src/lib.rs`

**Interfaces:**
- Consumes: 모든 Intent + Mode/Scope
- Produces: `DesiredState { mode, scope?, roles, channels, features }` (Default 파생).

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/state.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, ResourceKey};
    use crate::role::RoleIntent;

    #[test]
    fn empty_defaults_to_patch() {
        let json = r#"{}"#;
        let s: DesiredState = serde_json::from_str(json).unwrap();
        assert_eq!(s.mode, DesiredStateMode::Patch);
        assert!(s.roles.is_empty());
    }

    #[test]
    fn desired_state_roundtrip() {
        let s = DesiredState {
            mode: DesiredStateMode::Patch,
            scope: None,
            roles: vec![RoleIntent {
                identity: Identity { key: ResourceKey("r1".to_string()), ..Default::default() },
                name: Some("x".to_string()),
                permissions: None,
            }],
            channels: vec![],
            features: vec![],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<DesiredState>(&json).unwrap(), s);
    }
}
```

> `Identity { key, ..Default::default() }`가 되려면 `Identity`에 `Default`가 필요하다. Task 2의 `Identity`에 `#[derive(Default)]`를 추가하라(ResourceKey도 `Default` 파생 필요 — String wrapper라 자동). **Task 2 파일 수정 후 재커밋이 아니라, 이 Task의 커밋에 포함**시켜도 된다.

Modify `lib.rs`: `pub mod state;` + `pub use state::DesiredState;`.

- [ ] **Step 2: 실패 확인** — FAIL.

- [ ] **Step 3: 구현**

먼저 `identity.rs`의 `ResourceKey`와 `Identity`에 `Default` 추가:
```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceKey(pub String);
```
```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub key: ResourceKey,
    #[serde(rename = "match", default)]
    pub match_by: MatchStrategy,
    #[serde(default)]
    pub ownership: Ownership,
    #[serde(default)]
    pub state: ResourceState,
}
```

`state.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use crate::channel::ChannelIntent;
use crate::feature::FeatureIntent;
use crate::mode::{DesiredStateMode, Scope};
use crate::role::RoleIntent;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredState {
    #[serde(default)]
    pub mode: DesiredStateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleIntent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelIntent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<FeatureIntent>,
}
```

- [ ] **Step 4: 통과 확인** — PASS (18 tests).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add DesiredState root and Default derives"
```

---

### Task 9: 검증 규칙 1-3 (구조)

**Files:**
- Create: `crates/desired-state/src/validate.rs`
- Modify: `crates/desired-state/src/lib.rs`, `crates/desired-state/src/feature.rs`

**Interfaces:**
- Produces: `ValidationError`(enum), `DesiredState::validate() -> Result<(), Vec<ValidationError>>`, `FeatureIntent::identity() -> Option<&Identity>`. 규칙 1(key 유일성)·2(참조 무결성)·3(mode↔scope).

- [ ] **Step 1: 실패 테스트 작성**

Create `crates/desired-state/src/validate.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::channel::ChannelIntent;
    use crate::identity::{Identity, ResourceKey};
    use crate::mode::{DesiredStateMode, ResourceScope, Scope};
    use crate::role::RoleIntent;
    use crate::state::DesiredState;
    use crate::validate::ValidationError;

    fn role(key: &str) -> RoleIntent {
        RoleIntent {
            identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() },
            name: Some(key.to_string()),
            permissions: None,
        }
    }

    #[test]
    fn ok_when_valid() {
        let s = DesiredState { roles: vec![role("a")], ..Default::default() };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn duplicate_key_detected() {
        let s = DesiredState { roles: vec![role("a"), role("a")], ..Default::default() };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::DuplicateKey("a".to_string())));
    }

    #[test]
    fn dangling_reference_detected() {
        let ch = ChannelIntent {
            identity: Identity { key: ResourceKey("c".to_string()), ..Default::default() },
            name: Some("c".to_string()),
            channel_type: None,
            parent: Some(ResourceKey("missing".to_string())),
            access: None,
            raw_overwrites: None,
        };
        let s = DesiredState { channels: vec![ch], ..Default::default() };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::DanglingReference("missing".to_string())));
    }

    #[test]
    fn scope_mode_mismatch_detected() {
        let s = DesiredState {
            mode: DesiredStateMode::Patch,
            scope: Some(Scope { roles: Some(ResourceScope::All), channels: None }),
            ..Default::default()
        };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::ScopeWithoutScopedMode));
    }

    #[test]
    fn scoped_mode_requires_scope() {
        let s = DesiredState {
            mode: DesiredStateMode::ScopedAuthoritative,
            scope: None,
            ..Default::default()
        };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::ScopedModeWithoutScope));
    }
}
```

Modify `lib.rs`: `pub mod validate;` + `pub use validate::ValidationError;`.

Modify `feature.rs`: add helper (테스트 모듈 위, `impl FeatureIntent` 블록):
```rust
impl FeatureIntent {
    pub fn identity(&self) -> Option<&Identity> {
        match self {
            FeatureIntent::Verification(v) => Some(&v.identity),
            FeatureIntent::Moderation(_) | FeatureIntent::Logging(_) => None,
        }
    }
}
```

- [ ] **Step 2: 실패 확인** — FAIL.

- [ ] **Step 3: 구현**

`validate.rs` 테스트 위에:
```rust
use std::collections::BTreeSet;

use thiserror::Error;

use crate::access::OverwriteTargetIntent;
use crate::feature::FeatureIntent;
use crate::identity::ResourceKey;
use crate::mode::DesiredStateMode;
use crate::state::DesiredState;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("duplicate key: {0}")]
    DuplicateKey(String),
    #[error("dangling reference: {0}")]
    DanglingReference(String),
    #[error("scope present but mode is not scoped_authoritative")]
    ScopeWithoutScopedMode,
    #[error("scoped_authoritative mode requires a scope")]
    ScopedModeWithoutScope,
}

impl DesiredState {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        self.check_key_uniqueness(&mut errors);
        self.check_reference_integrity(&mut errors);
        self.check_mode_scope(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn declared_keys(&self) -> Vec<&ResourceKey> {
        let mut keys = Vec::new();
        for r in &self.roles {
            keys.push(&r.identity.key);
        }
        for c in &self.channels {
            keys.push(&c.identity.key);
        }
        for f in &self.features {
            if let Some(id) = f.identity() {
                keys.push(&id.key);
            }
        }
        keys
    }

    fn check_key_uniqueness(&self, errors: &mut Vec<ValidationError>) {
        let mut seen = BTreeSet::new();
        for key in self.declared_keys() {
            if !seen.insert(key.clone()) {
                errors.push(ValidationError::DuplicateKey(key.0.clone()));
            }
        }
    }

    fn check_reference_integrity(&self, errors: &mut Vec<ValidationError>) {
        let declared: BTreeSet<ResourceKey> =
            self.declared_keys().into_iter().cloned().collect();
        let mut refs: Vec<&ResourceKey> = Vec::new();
        for c in &self.channels {
            if let Some(p) = &c.parent {
                refs.push(p);
            }
            if let Some(access) = &c.access {
                refs.extend(access.roles.keys());
            }
            if let Some(raws) = &c.raw_overwrites {
                for r in raws {
                    if let OverwriteTargetIntent::Role(k) = &r.target {
                        refs.push(k);
                    }
                }
            }
        }
        for f in &self.features {
            if let FeatureIntent::Verification(v) = f {
                refs.push(&v.channel);
                refs.push(&v.grants_role);
            }
        }
        for k in refs {
            if !declared.contains(k) {
                errors.push(ValidationError::DanglingReference(k.0.clone()));
            }
        }
    }

    fn check_mode_scope(&self, errors: &mut Vec<ValidationError>) {
        match (self.mode, self.scope.is_some()) {
            (DesiredStateMode::ScopedAuthoritative, false) => {
                errors.push(ValidationError::ScopedModeWithoutScope);
            }
            (DesiredStateMode::ScopedAuthoritative, true) => {}
            (_, true) => errors.push(ValidationError::ScopeWithoutScopedMode),
            (_, false) => {}
        }
    }
}
```

- [ ] **Step 4: 통과 확인** — `cargo test -p desired-state` → PASS (23 tests). `cargo clippy --all-targets -- -D warnings` 통과(모든 ValidationError variant가 사용됨).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add validate rules 1-3 (structural)"
```

---

### Task 10: 검증 규칙 4-6 (의미)

**Files:**
- Modify: `crates/desired-state/src/validate.rs`

**Interfaces:**
- Produces: 규칙 4(ownership↔state), 5(match↔name), 6(access↔raw 충돌) + `ValidationError` 변형 추가.

- [ ] **Step 1: 실패 테스트 작성**

`validate.rs` 테스트 모듈에 추가:
```rust
    use crate::access::{AccessGrant, AccessIntent, Capability, OverwriteOp, OverwriteTargetIntent as OT, PermissionOverwriteIntent};
    use crate::identity::{MatchStrategy, Ownership, ResourceState};
    use discord_model::Permissions;

    #[test]
    fn referenced_cannot_be_mutated() {
        let r = RoleIntent {
            identity: Identity {
                key: ResourceKey("r".to_string()),
                ownership: Ownership::Referenced,
                ..Default::default()
            },
            name: Some("r".to_string()),
            permissions: Some(Permissions::empty()),
        };
        let s = DesiredState { roles: vec![r], ..Default::default() };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::ReferencedNotMutable("r".to_string())));
    }

    #[test]
    fn referenced_cannot_be_absent() {
        let r = RoleIntent {
            identity: Identity {
                key: ResourceKey("r".to_string()),
                ownership: Ownership::Referenced,
                state: ResourceState::Absent,
                ..Default::default()
            },
            name: None,
            permissions: None,
        };
        let s = DesiredState { roles: vec![r], ..Default::default() };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::AbsentRequiresOwnership("r".to_string())));
    }

    #[test]
    fn match_by_name_requires_name() {
        let r = RoleIntent {
            identity: Identity {
                key: ResourceKey("r".to_string()),
                match_by: MatchStrategy::ByName,
                ..Default::default()
            },
            name: None,
            permissions: None,
        };
        let s = DesiredState { roles: vec![r], ..Default::default() };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::MatchByNameRequiresName("r".to_string())));
    }

    #[test]
    fn access_raw_conflict_detected() {
        let key = ResourceKey("verified".to_string());
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(key.clone(), AccessGrant { allow: vec![Capability::View], deny: vec![] });
        let ch = ChannelIntent {
            identity: Identity { key: ResourceKey("c".to_string()), ..Default::default() },
            name: Some("c".to_string()),
            channel_type: None,
            parent: None,
            access: Some(AccessIntent { everyone: None, roles }),
            raw_overwrites: Some(vec![PermissionOverwriteIntent {
                target: OT::Role(key.clone()),
                op: OverwriteOp::Add,
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }]),
        };
        let refd = RoleIntent {
            identity: Identity { key, ownership: Ownership::Referenced, ..Default::default() },
            name: None,
            permissions: None,
        };
        let s = DesiredState { roles: vec![refd], channels: vec![ch], ..Default::default() };
        let err = s.validate().unwrap_err();
        assert!(err.contains(&ValidationError::AccessRawConflict("c".to_string())));
    }
```

- [ ] **Step 2: 실패 확인** — FAIL (미정의 variant + 규칙 미구현).

- [ ] **Step 3: 구현**

`validate.rs`의 `ValidationError`에 variant 추가:
```rust
    #[error("referenced resource cannot be modified: {0}")]
    ReferencedNotMutable(String),
    #[error("absent state requires managed or adopted ownership: {0}")]
    AbsentRequiresOwnership(String),
    #[error("match by_name requires a name: {0}")]
    MatchByNameRequiresName(String),
    #[error("access and raw overwrite target the same role in channel: {0}")]
    AccessRawConflict(String),
```

import 추가(파일 상단 use 블록):
```rust
use crate::identity::{Identity, MatchStrategy, Ownership, ResourceState};
```

`validate()` 본문에 호출 3개 추가(mode_scope 뒤):
```rust
        self.check_ownership_state(&mut errors);
        self.check_match_name(&mut errors);
        self.check_access_raw_conflict(&mut errors);
```

`impl DesiredState`에 메서드 추가:
```rust
    fn check_ownership_state(&self, errors: &mut Vec<ValidationError>) {
        for r in &self.roles {
            let mutated = r.name.is_some() || r.permissions.is_some();
            Self::check_one_ownership_state(&r.identity, mutated, errors);
        }
        for c in &self.channels {
            let mutated = c.name.is_some()
                || c.channel_type.is_some()
                || c.parent.is_some()
                || c.access.is_some()
                || c.raw_overwrites.is_some();
            Self::check_one_ownership_state(&c.identity, mutated, errors);
        }
    }

    fn check_one_ownership_state(
        id: &Identity,
        mutated: bool,
        errors: &mut Vec<ValidationError>,
    ) {
        if id.ownership == Ownership::Referenced && mutated {
            errors.push(ValidationError::ReferencedNotMutable(id.key.0.clone()));
        }
        if id.state == ResourceState::Absent && id.ownership == Ownership::Referenced {
            errors.push(ValidationError::AbsentRequiresOwnership(id.key.0.clone()));
        }
    }

    fn check_match_name(&self, errors: &mut Vec<ValidationError>) {
        for r in &self.roles {
            if r.identity.match_by == MatchStrategy::ByName && r.name.is_none() {
                errors.push(ValidationError::MatchByNameRequiresName(r.identity.key.0.clone()));
            }
        }
        for c in &self.channels {
            if c.identity.match_by == MatchStrategy::ByName && c.name.is_none() {
                errors.push(ValidationError::MatchByNameRequiresName(c.identity.key.0.clone()));
            }
        }
    }

    fn check_access_raw_conflict(&self, errors: &mut Vec<ValidationError>) {
        for c in &self.channels {
            let (Some(access), Some(raws)) = (&c.access, &c.raw_overwrites) else {
                continue;
            };
            let access_roles: BTreeSet<&ResourceKey> = access.roles.keys().collect();
            for r in raws {
                if let OverwriteTargetIntent::Role(k) = &r.target {
                    if access_roles.contains(k) {
                        errors.push(ValidationError::AccessRawConflict(c.identity.key.0.clone()));
                        break;
                    }
                }
            }
        }
    }
```

> 참고: 규칙 4/5는 role·channel에 적용(feature는 대상 아님). `MatchStrategy::ByName == ...` 비교는 파생된 `PartialEq` 사용.

- [ ] **Step 4: 통과 확인** — `cargo test -p desired-state` → PASS (27 tests). clippy 통과(신규 variant 전부 사용됨).

- [ ] **Step 5: 커밋**
```bash
cargo fmt --all
git add -A
git commit -m "feat(desired-state): add validate rules 4-6 (semantic)"
```

---

### Task 11: 인증 시나리오 통합 픽스처 + 최종 게이트

**Files:**
- Create: `crates/desired-state/tests/verification_scenario.rs`

**Interfaces:**
- Consumes: `desired_state::*`
- Produces: 문서 §6.2 시나리오를 `DesiredState`로 구성 → `validate()` 통과 + serde 라운드트립 (이후 Diff 테스트의 입력 픽스처).

- [ ] **Step 1: 통합 테스트 작성**

Create `crates/desired-state/tests/verification_scenario.rs`:
```rust
use std::collections::BTreeMap;

use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, DesiredStateMode,
    FeatureIntent, Identity, Ownership, ResourceKey, RoleIntent, VerificationIntent,
};
use discord_model::{ChannelType, Permissions};

#[test]
fn verification_scenario_validates_and_roundtrips() {
    let verified = ResourceKey("verified_member".to_string());
    let verify_ch = ResourceKey("verification_channel".to_string());
    let general_ch = ResourceKey("general_channel".to_string());

    let role = RoleIntent {
        identity: Identity { key: verified.clone(), ..Default::default() },
        name: Some("인증됨".to_string()),
        permissions: Some(Permissions::empty()),
    };

    let verification_channel = ChannelIntent {
        identity: Identity { key: verify_ch.clone(), ..Default::default() },
        name: Some("인증".to_string()),
        channel_type: Some(ChannelType::Text),
        parent: None,
        access: Some(AccessIntent {
            everyone: Some(AccessGrant { allow: vec![Capability::View], deny: vec![] }),
            roles: BTreeMap::new(),
        }),
        raw_overwrites: None,
    };

    let mut general_roles = BTreeMap::new();
    general_roles.insert(
        verified.clone(),
        AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
    );
    let general_channel = ChannelIntent {
        identity: Identity { key: general_ch.clone(), ..Default::default() },
        name: Some("일반".to_string()),
        channel_type: Some(ChannelType::Text),
        parent: None,
        access: Some(AccessIntent {
            everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
            roles: general_roles,
        }),
        raw_overwrites: None,
    };

    let panel = FeatureIntent::Verification(VerificationIntent {
        identity: Identity {
            key: ResourceKey("verify_panel".to_string()),
            ownership: Ownership::Managed,
            ..Default::default()
        },
        channel: verify_ch,
        grants_role: verified,
    });

    let state = DesiredState {
        mode: DesiredStateMode::Patch,
        scope: None,
        roles: vec![role],
        channels: vec![verification_channel, general_channel],
        features: vec![panel],
    };

    assert!(state.validate().is_ok());

    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<DesiredState>(&json).unwrap(), state);
}
```

- [ ] **Step 2: 통과 확인**

Run: `cargo test -p desired-state --test verification_scenario`
Expected: 컴파일·통과. (모든 타입 존재하므로 바로 통과 가능 → 통과하면 OK.) re-export 누락 시 `lib.rs` 보완.

- [ ] **Step 3: 최종 품질 게이트**

Run:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. `cargo test`는 desired-state 27 + 통합 1 + (Phase 1) 18 = 총 46 통과.

- [ ] **Step 4: 커밋**
```bash
git add -A
git commit -m "test(desired-state): add verification scenario integration fixture"
```

---

## 완료 정의 (Definition of Done)

- [ ] 워크스페이스 컴파일 (`cargo build`)
- [ ] 전체 테스트 통과 (`cargo test` — desired-state 27 + 통합 1 + Phase 1 18 = 46)
- [ ] `cargo clippy --all-targets -- -D warnings` 경고 0
- [ ] `cargo fmt --all -- --check` diff 0
- [ ] 타입: DesiredState/Mode/Scope, 정체성(ResourceKey/MatchStrategy/Ownership/ResourceState/Identity), RoleIntent, ChannelIntent(+AccessIntent/Capability/raw escape), FeatureIntent(Verification+스켈레톤)
- [ ] `validate()` 6규칙 전부 + 규칙별 위반/통과 테스트
- [ ] 인증 시나리오 픽스처: validate 통과 + 라운드트립
- [ ] `desired-state → discord-model` 단방향, sqlx 없음, 주석 없음
- [ ] 각 Task별 커밋
