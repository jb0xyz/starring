# Desired Compiler Implementation Plan (Phase 3)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task를 크게 잡았으니, 각 Task 안에서 여러 TDD 사이클·커밋을 돌리고 Task 끝에 보고한다.

**Goal:** `crates/desired-compiler` — `DesiredState → NormalizedDesiredState` 하강(normalize) + 선행으로 discord-model Permissions 확장, desired-state Capability 확장·규칙6 제거.

**Architecture:** 순수 Rust 변환. 고수준 Capability→Discord Permission 매핑, AccessIntent→NormalizedOverwrite 하강, raw escape 병합(add/remove/replace), 문서 내부 충돌(`allow & deny` 겹침) 감지. resolve/DB/binding 없음.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), thiserror, bitflags(discord-model), desired-state·discord-model(path deps).

## Global Constraints
> ⚠️ **주석 금지**: `//`, `///`, `//!` 없음. 코드 블록의 설명 문구 제거하고 구현.
- 의존 방향 `desired-compiler → desired-state → discord-model`. diff/db/operation-graph/policy/simulator/bot/ai 의존 금지.
- 비트 연산은 모호성 회피 위해 Remove/충돌 판정에서 `.bits()` 명시적 u64 연산 사용(아래 코드대로).
- 결정적 출력: `NormalizedTarget`에 `Ord`, 누적은 `BTreeMap`.
- 완료 게이트: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
- Task별 여러 커밋 가능, Task 끝에 보고.

---

### Task 1: 선행 수정 (discord-model + desired-state)

**Files:**
- Modify: `crates/discord-model/src/permissions.rs`
- Modify: `crates/desired-state/src/access.rs`, `crates/desired-state/src/validate.rs`

**Interfaces:**
- Produces: `Permissions`에 6비트 추가. `Capability` = 9종(비-non_exhaustive). `validate()` 5규칙(규칙6 제거).

- [ ] **Step 1: discord-model 신규 비트 테스트**

`crates/discord-model/src/permissions.rs` 테스트 모듈에 추가:
```rust
    #[test]
    fn new_permission_bits() {
        assert_eq!(Permissions::ADD_REACTIONS.bits(), 1 << 6);
        assert_eq!(Permissions::EMBED_LINKS.bits(), 1 << 14);
        assert_eq!(Permissions::ATTACH_FILES.bits(), 1 << 15);
        assert_eq!(Permissions::READ_MESSAGE_HISTORY.bits(), 1 << 16);
        assert_eq!(Permissions::CONNECT.bits(), 1 << 20);
        assert_eq!(Permissions::SPEAK.bits(), 1 << 21);
    }
```

- [ ] **Step 2: 실패 확인** — `cargo test -p discord-model` → FAIL(미정의 상수).

- [ ] **Step 3: Permissions 블록 교체**

`permissions.rs`의 `bitflags! { ... }` 전체를 아래로 교체(비트 순서 유지):
```rust
bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Permissions: u64 {
        const CREATE_INSTANT_INVITE = 1 << 0;
        const KICK_MEMBERS          = 1 << 1;
        const BAN_MEMBERS           = 1 << 2;
        const ADMINISTRATOR         = 1 << 3;
        const MANAGE_CHANNELS       = 1 << 4;
        const MANAGE_GUILD          = 1 << 5;
        const ADD_REACTIONS         = 1 << 6;
        const VIEW_CHANNEL          = 1 << 10;
        const SEND_MESSAGES         = 1 << 11;
        const MANAGE_MESSAGES       = 1 << 13;
        const EMBED_LINKS           = 1 << 14;
        const ATTACH_FILES          = 1 << 15;
        const READ_MESSAGE_HISTORY  = 1 << 16;
        const CONNECT               = 1 << 20;
        const SPEAK                 = 1 << 21;
        const MANAGE_ROLES          = 1 << 28;
    }
}
```
(값은 공식 Discord permissions 문서로 검증.)

- [ ] **Step 4: 통과 + 커밋**
```bash
cargo test -p discord-model && cargo fmt --all
git add -A
git commit -m "feat(discord-model): add text/voice permission bits"
```

- [ ] **Step 5: desired-state Capability 변경 (테스트 먼저)**

`crates/desired-state/src/access.rs`의 `capability_serde` 테스트를 아래로 교체:
```rust
    #[test]
    fn capability_serde() {
        assert_eq!(serde_json::to_string(&Capability::View).unwrap(), r#""view""#);
        assert_eq!(serde_json::to_string(&Capability::ReadHistory).unwrap(), r#""read_history""#);
        assert_eq!(serde_json::to_string(&Capability::AddReactions).unwrap(), r#""add_reactions""#);
        assert_eq!(serde_json::to_string(&Capability::EmbedLinks).unwrap(), r#""embed_links""#);
    }
```

- [ ] **Step 6: 실패 확인** — `cargo test -p desired-state` → FAIL(ReadHistory 등 미정의).

- [ ] **Step 7: Capability enum 교체**

`access.rs`의 `Capability` 정의를 아래로 교체(`#[non_exhaustive]` 제거 — Compiler가 exhaustive match로 매핑 누락을 컴파일타임에 잡게):
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    View,
    Send,
    ReadHistory,
    AddReactions,
    AttachFiles,
    EmbedLinks,
    ManageMessages,
    Connect,
    Speak,
}
```

- [ ] **Step 8: 규칙6 제거**

`crates/desired-state/src/validate.rs`에서:
- `validate()` 본문의 `self.check_access_raw_conflict(&mut errors);` 호출 줄 제거.
- `check_access_raw_conflict` 메서드 전체 제거.
- `ValidationError::AccessRawConflict { ... }` variant 제거.
- 테스트 모듈의 `access_raw_conflict_detected` 테스트 제거.
- 그로 인해 unused가 되는 import(예: `OverwriteTargetIntent as OT`, `AccessGrant`/`AccessIntent`/`Capability`가 다른 테스트에서 안 쓰이면) 정리 — `cargo clippy`가 잡는다.

- [ ] **Step 9: 통과 + 커밋**
```bash
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(desired-state): expand Capability set, drop coarse access-raw rule"
```

- [ ] **Step 10: Task 보고** — 커밋 2개 / discord-model·desired-state 테스트 결과 / 게이트.

---

### Task 2: desired-compiler 스캐폴드 + Normalized 타입 + capability 매핑

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/desired-compiler/Cargo.toml`, `src/lib.rs`, `src/normalized.rs`, `src/capability.rs`

**Interfaces:**
- Produces: crate `desired_compiler`. 타입 `NormalizedDesiredState/NormalizedRole/NormalizedChannel/NormalizedOverwrite/NormalizedTarget/NormalizedVerificationPanel`. 함수 `capability_to_permission`, `capabilities_to_permissions`.

- [ ] **Step 1: 워크스페이스 등록 + crate 파일**

Root `Cargo.toml` members에 `"crates/desired-compiler"` 추가.

Create `crates/desired-compiler/Cargo.toml`:
```toml
[package]
name = "desired-compiler"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }
desired-state = { path = "../desired-state" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/desired-compiler/src/lib.rs`:
```rust
pub mod capability;
pub mod normalized;

pub use capability::{capabilities_to_permissions, capability_to_permission};
pub use normalized::{
    NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
    NormalizedTarget, NormalizedVerificationPanel,
};
```

- [ ] **Step 2: capability 매핑 테스트**

Create `crates/desired-compiler/src/capability.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::Capability;
    use discord_model::Permissions;

    #[test]
    fn maps_each_capability() {
        assert_eq!(capability_to_permission(Capability::View), Permissions::VIEW_CHANNEL);
        assert_eq!(capability_to_permission(Capability::Send), Permissions::SEND_MESSAGES);
        assert_eq!(capability_to_permission(Capability::ReadHistory), Permissions::READ_MESSAGE_HISTORY);
        assert_eq!(capability_to_permission(Capability::Speak), Permissions::SPEAK);
    }

    #[test]
    fn unions_capabilities() {
        let p = capabilities_to_permissions(&[Capability::View, Capability::Send]);
        assert_eq!(p, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES);
    }
}
```

- [ ] **Step 3: capability 매핑 구현**

`capability.rs` 테스트 위에:
```rust
use desired_state::Capability;
use discord_model::Permissions;

pub fn capability_to_permission(cap: Capability) -> Permissions {
    match cap {
        Capability::View => Permissions::VIEW_CHANNEL,
        Capability::Send => Permissions::SEND_MESSAGES,
        Capability::ReadHistory => Permissions::READ_MESSAGE_HISTORY,
        Capability::AddReactions => Permissions::ADD_REACTIONS,
        Capability::AttachFiles => Permissions::ATTACH_FILES,
        Capability::EmbedLinks => Permissions::EMBED_LINKS,
        Capability::ManageMessages => Permissions::MANAGE_MESSAGES,
        Capability::Connect => Permissions::CONNECT,
        Capability::Speak => Permissions::SPEAK,
    }
}

pub fn capabilities_to_permissions(caps: &[Capability]) -> Permissions {
    caps.iter()
        .fold(Permissions::empty(), |acc, &c| acc | capability_to_permission(c))
}
```

- [ ] **Step 4: Normalized 타입 테스트**

Create `crates/desired-compiler/src/normalized.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::ResourceKey;
    use discord_model::Permissions;

    #[test]
    fn normalized_target_serde() {
        let t = NormalizedTarget::Everyone;
        assert_eq!(serde_json::to_string(&t).unwrap(), r#"{"target":"everyone"}"#);
        let r = NormalizedTarget::Role(ResourceKey("verified".to_string()));
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"target":"role","id":"verified"}"#);
        assert_eq!(serde_json::from_str::<NormalizedTarget>(&serde_json::to_string(&r).unwrap()).unwrap(), r);
    }

    #[test]
    fn normalized_state_roundtrip() {
        let s = NormalizedDesiredState::default();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<NormalizedDesiredState>(&json).unwrap(), s);
        let ow = NormalizedOverwrite {
            target: NormalizedTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        };
        let json = serde_json::to_string(&ow).unwrap();
        assert_eq!(serde_json::from_str::<NormalizedOverwrite>(&json).unwrap(), ow);
    }
}
```

- [ ] **Step 5: Normalized 타입 구현**

`normalized.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use desired_state::{DesiredStateMode, Identity, ResourceKey, Scope};
use discord_model::{ChannelType, Permissions};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedDesiredState {
    pub mode: DesiredStateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<NormalizedRole>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<NormalizedChannel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_panels: Vec<NormalizedVerificationPanel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedRole {
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedChannel {
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_type: Option<ChannelType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ResourceKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overwrites: Vec<NormalizedOverwrite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedOverwrite {
    pub target: NormalizedTarget,
    pub allow: Permissions,
    pub deny: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "target", content = "id", rename_all = "snake_case")]
pub enum NormalizedTarget {
    Everyone,
    Role(ResourceKey),
    Member(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedVerificationPanel {
    pub identity: Identity,
    pub channel: ResourceKey,
    pub grants_role: ResourceKey,
}
```

- [ ] **Step 6: 통과 확인 + 커밋**
```bash
cargo test -p desired-compiler && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(desired-compiler): scaffold crate with normalized types and capability mapping"
```

- [ ] **Step 7: Task 보고** — 커밋 / 테스트 / 게이트.

---

### Task 3: compile() — 하강·병합·충돌·passthrough

**Files:**
- Create: `crates/desired-compiler/src/error.rs`, `crates/desired-compiler/src/compile.rs`
- Modify: `crates/desired-compiler/src/lib.rs`

**Interfaces:**
- Consumes: desired-state 타입 전체, `capabilities_to_permissions`, Normalized 타입
- Produces: `CompileError`, `compile(&DesiredState) -> Result<NormalizedDesiredState, Vec<CompileError>>`.

- [ ] **Step 1: compile 테스트 작성**

Create `crates/desired-compiler/src/compile.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CompileError;
    use crate::normalized::NormalizedTarget;
    use desired_state::{
        AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, FeatureIntent,
        Identity, OverwriteOp, OverwriteTargetIntent, PermissionOverwriteIntent, ResourceKey,
        RoleIntent, VerificationIntent,
    };
    use discord_model::Permissions;
    use std::collections::BTreeMap;

    fn channel_with(access: Option<AccessIntent>, raw: Option<Vec<PermissionOverwriteIntent>>) -> ChannelIntent {
        ChannelIntent {
            identity: Identity { key: ResourceKey("c".to_string()), ..Default::default() },
            name: Some("c".to_string()),
            channel_type: None,
            parent: None,
            access,
            raw_overwrites: raw,
        }
    }

    fn find<'a>(nc: &'a crate::normalized::NormalizedChannel, t: &NormalizedTarget) -> &'a crate::normalized::NormalizedOverwrite {
        nc.overwrites.iter().find(|o| &o.target == t).unwrap()
    }

    #[test]
    fn lowers_everyone_and_role_access() {
        let mut roles = BTreeMap::new();
        roles.insert(
            ResourceKey("verified".to_string()),
            AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
        );
        let access = AccessIntent {
            everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
            roles,
        };
        let ds = DesiredState { channels: vec![channel_with(Some(access), None)], ..Default::default() };
        let out = compile(&ds).unwrap();
        let ch = &out.channels[0];
        let everyone = find(ch, &NormalizedTarget::Everyone);
        assert_eq!(everyone.deny, Permissions::VIEW_CHANNEL);
        let verified = find(ch, &NormalizedTarget::Role(ResourceKey("verified".to_string())));
        assert_eq!(verified.allow, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES);
    }

    #[test]
    fn raw_add_and_remove_merge() {
        let raw = vec![
            PermissionOverwriteIntent {
                target: OverwriteTargetIntent::Role(ResourceKey("verified".to_string())),
                op: OverwriteOp::Add,
                allow: Permissions::EMBED_LINKS,
                deny: Permissions::empty(),
            },
        ];
        let mut roles = BTreeMap::new();
        roles.insert(ResourceKey("verified".to_string()), AccessGrant { allow: vec![Capability::View], deny: vec![] });
        let access = AccessIntent { everyone: None, roles };
        let ds = DesiredState { channels: vec![channel_with(Some(access), Some(raw))], ..Default::default() };
        let out = compile(&ds).unwrap();
        let v = find(&out.channels[0], &NormalizedTarget::Role(ResourceKey("verified".to_string())));
        assert_eq!(v.allow, Permissions::VIEW_CHANNEL | Permissions::EMBED_LINKS);
    }

    #[test]
    fn raw_replace_overrides() {
        let raw = vec![PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified".to_string())),
            op: OverwriteOp::Replace,
            allow: Permissions::SPEAK,
            deny: Permissions::empty(),
        }];
        let mut roles = BTreeMap::new();
        roles.insert(ResourceKey("verified".to_string()), AccessGrant { allow: vec![Capability::View], deny: vec![] });
        let ds = DesiredState { channels: vec![channel_with(Some(AccessIntent { everyone: None, roles }), Some(raw))], ..Default::default() };
        let out = compile(&ds).unwrap();
        let v = find(&out.channels[0], &NormalizedTarget::Role(ResourceKey("verified".to_string())));
        assert_eq!(v.allow, Permissions::SPEAK);
    }

    #[test]
    fn conflict_when_allow_and_deny_overlap() {
        let raw = vec![PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified".to_string())),
            op: OverwriteOp::Add,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        }];
        let mut roles = BTreeMap::new();
        roles.insert(ResourceKey("verified".to_string()), AccessGrant { allow: vec![Capability::View], deny: vec![] });
        let ds = DesiredState { channels: vec![channel_with(Some(AccessIntent { everyone: None, roles }), Some(raw))], ..Default::default() };
        let err = compile(&ds).unwrap_err();
        assert!(matches!(err[0], CompileError::PermissionConflict { .. }));
    }

    #[test]
    fn passthrough_roles_verification_mode() {
        let ds = DesiredState {
            roles: vec![RoleIntent {
                identity: Identity { key: ResourceKey("r".to_string()), ..Default::default() },
                name: Some("r".to_string()),
                permissions: Some(Permissions::empty()),
            }],
            features: vec![
                FeatureIntent::Verification(VerificationIntent {
                    identity: Identity { key: ResourceKey("p".to_string()), ..Default::default() },
                    channel: ResourceKey("c".to_string()),
                    grants_role: ResourceKey("r".to_string()),
                }),
                FeatureIntent::Moderation(Default::default()),
            ],
            ..Default::default()
        };
        let out = compile(&ds).unwrap();
        assert_eq!(out.roles.len(), 1);
        assert_eq!(out.verification_panels.len(), 1);
    }
}
```

Modify `lib.rs`: `pub mod compile; pub mod error;` + `pub use compile::compile; pub use error::CompileError;`.

- [ ] **Step 2: 실패 확인** — `cargo test -p desired-compiler` → FAIL.

- [ ] **Step 3: error.rs 구현**

Create `crates/desired-compiler/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CompileError {
    #[error("permission conflict in channel {channel} for target {target}")]
    PermissionConflict { channel: String, target: String },
}
```

- [ ] **Step 4: compile.rs 구현**

`compile.rs` 테스트 위에:
```rust
use std::collections::BTreeMap;

use desired_state::{
    AccessGrant, ChannelIntent, DesiredState, FeatureIntent, OverwriteOp, OverwriteTargetIntent,
    PermissionOverwriteIntent, RoleIntent, VerificationIntent,
};
use discord_model::Permissions;

use crate::capability::capabilities_to_permissions;
use crate::error::CompileError;
use crate::normalized::{
    NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
    NormalizedTarget, NormalizedVerificationPanel,
};

pub fn compile(desired: &DesiredState) -> Result<NormalizedDesiredState, Vec<CompileError>> {
    let mut errors = Vec::new();

    let roles = desired.roles.iter().map(normalize_role).collect();
    let verification_panels = desired.features.iter().filter_map(normalize_feature).collect();

    let mut channels = Vec::new();
    for c in &desired.channels {
        match normalize_channel(c) {
            Ok(nc) => channels.push(nc),
            Err(mut e) => errors.append(&mut e),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NormalizedDesiredState {
        mode: desired.mode,
        scope: desired.scope.clone(),
        roles,
        channels,
        verification_panels,
    })
}

fn normalize_role(r: &RoleIntent) -> NormalizedRole {
    NormalizedRole {
        identity: r.identity.clone(),
        name: r.name.clone(),
        permissions: r.permissions,
    }
}

fn normalize_feature(f: &FeatureIntent) -> Option<NormalizedVerificationPanel> {
    match f {
        FeatureIntent::Verification(v) => Some(normalize_verification(v)),
        FeatureIntent::Moderation(_) | FeatureIntent::Logging(_) => None,
    }
}

fn normalize_verification(v: &VerificationIntent) -> NormalizedVerificationPanel {
    NormalizedVerificationPanel {
        identity: v.identity.clone(),
        channel: v.channel.clone(),
        grants_role: v.grants_role.clone(),
    }
}

fn normalize_channel(c: &ChannelIntent) -> Result<NormalizedChannel, Vec<CompileError>> {
    let mut map: BTreeMap<NormalizedTarget, (Permissions, Permissions)> = BTreeMap::new();

    if let Some(access) = &c.access {
        if let Some(grant) = &access.everyone {
            apply_grant(map.entry(NormalizedTarget::Everyone).or_default(), grant);
        }
        for (key, grant) in &access.roles {
            apply_grant(map.entry(NormalizedTarget::Role(key.clone())).or_default(), grant);
        }
    }

    if let Some(raws) = &c.raw_overwrites {
        for raw in raws {
            apply_raw(map.entry(raw_target(&raw.target)).or_default(), raw);
        }
    }

    let mut errors = Vec::new();
    let mut overwrites = Vec::new();
    for (target, (allow, deny)) in map {
        if allow.bits() & deny.bits() != 0 {
            errors.push(CompileError::PermissionConflict {
                channel: c.identity.key.0.clone(),
                target: target_label(&target),
            });
            continue;
        }
        overwrites.push(NormalizedOverwrite { target, allow, deny });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NormalizedChannel {
        identity: c.identity.clone(),
        name: c.name.clone(),
        channel_type: c.channel_type,
        parent: c.parent.clone(),
        overwrites,
    })
}

fn apply_grant(entry: &mut (Permissions, Permissions), grant: &AccessGrant) {
    entry.0 |= capabilities_to_permissions(&grant.allow);
    entry.1 |= capabilities_to_permissions(&grant.deny);
}

fn apply_raw(entry: &mut (Permissions, Permissions), raw: &PermissionOverwriteIntent) {
    match raw.op {
        OverwriteOp::Add => {
            entry.0 |= raw.allow;
            entry.1 |= raw.deny;
        }
        OverwriteOp::Remove => {
            entry.0 = Permissions::from_bits_retain(entry.0.bits() & !raw.allow.bits());
            entry.1 = Permissions::from_bits_retain(entry.1.bits() & !raw.deny.bits());
        }
        OverwriteOp::Replace => {
            entry.0 = raw.allow;
            entry.1 = raw.deny;
        }
    }
}

fn raw_target(t: &OverwriteTargetIntent) -> NormalizedTarget {
    match t {
        OverwriteTargetIntent::Role(k) => NormalizedTarget::Role(k.clone()),
        OverwriteTargetIntent::Member(id) => NormalizedTarget::Member(id.clone()),
    }
}

fn target_label(t: &NormalizedTarget) -> String {
    match t {
        NormalizedTarget::Everyone => "everyone".to_string(),
        NormalizedTarget::Role(k) => k.0.clone(),
        NormalizedTarget::Member(id) => id.clone(),
    }
}
```

> `or_default()`는 Permissions의 Default(Task 1에서 추가, empty)에 의존한다. 혹시 bitflags 버전이 Default 파생을 지원 안 해 컴파일이 깨지면 `.or_insert_with(|| (Permissions::empty(), Permissions::empty()))`로 바꿔라(명시된 대안).

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p desired-compiler && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(desired-compiler): implement compile with lowering, merge, and conflict detection"
```

- [ ] **Step 6: Task 보고** — 커밋 / 테스트 5개 / 게이트 / (or_default vs or_insert_with 중 뭘 썼는지).

---

### Task 4: 인증 시나리오 통합 픽스처 + 최종 게이트

**Files:**
- Create: `crates/desired-compiler/tests/verification_scenario.rs`

**Interfaces:**
- Produces: Phase 2 인증 DesiredState → `compile()` → NormalizedDesiredState 검증 (Phase 4 Diff 입력).

- [ ] **Step 1: 통합 테스트 작성**

Create `crates/desired-compiler/tests/verification_scenario.rs`:
```rust
use std::collections::BTreeMap;

use desired_compiler::{compile, NormalizedTarget};
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, DesiredStateMode,
    FeatureIntent, Identity, ResourceKey, RoleIntent, VerificationIntent,
};
use discord_model::{ChannelType, Permissions};

#[test]
fn compiles_verification_scenario() {
    let verified = ResourceKey("verified_member".to_string());

    let general = {
        let mut roles = BTreeMap::new();
        roles.insert(
            verified.clone(),
            AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
        );
        ChannelIntent {
            identity: Identity { key: ResourceKey("general_channel".to_string()), ..Default::default() },
            name: Some("일반".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
                roles,
            }),
            raw_overwrites: None,
        }
    };

    let ds = DesiredState {
        mode: DesiredStateMode::Patch,
        scope: None,
        roles: vec![RoleIntent {
            identity: Identity { key: verified.clone(), ..Default::default() },
            name: Some("인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![general],
        features: vec![FeatureIntent::Verification(VerificationIntent {
            identity: Identity { key: ResourceKey("panel".to_string()), ..Default::default() },
            channel: ResourceKey("verification_channel".to_string()),
            grants_role: verified.clone(),
        })],
    };

    let out = compile(&ds).unwrap();

    let ch = &out.channels[0];
    let everyone = ch.overwrites.iter().find(|o| o.target == NormalizedTarget::Everyone).unwrap();
    assert_eq!(everyone.deny, Permissions::VIEW_CHANNEL);
    let v = ch.overwrites.iter().find(|o| o.target == NormalizedTarget::Role(verified.clone())).unwrap();
    assert_eq!(v.allow, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES);

    assert_eq!(out.roles.len(), 1);
    assert_eq!(out.verification_panels.len(), 1);
    assert_eq!(out.mode, DesiredStateMode::Patch);
}
```

- [ ] **Step 2: 통과 확인** — `cargo test -p desired-compiler --test verification_scenario`. re-export 누락 시 lib.rs 보완.

- [ ] **Step 3: 최종 품질 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 ≈ 55 전후 (Phase1 discord-model 비트 테스트 +1, desired-state 규칙6 테스트 -1, desired-compiler 단위 9 + 통합 1). 정확한 수는 실제 출력대로 보고.

- [ ] **Step 4: 커밋 + 보고**
```bash
git add -A
git commit -m "test(desired-compiler): add verification scenario integration fixture"
```
최종 보고: 커밋 / 총 테스트 수 / 4게이트 / Definition of Done.

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build` / `cargo test`(전 crate) / `cargo clippy --all-targets -- -D warnings`(0) / `cargo fmt --all -- --check`(0)
- [ ] discord-model: 신규 6비트 + 값 검증
- [ ] desired-state: Capability 9종(non_exhaustive 제거), 규칙6 제거(validate 5규칙)
- [ ] desired-compiler: Normalized 타입, capability 매핑, `compile()`(하강·병합 add/remove/replace·충돌·passthrough), CompileError
- [ ] 인증 시나리오: compile 성공 + normalized 출력 검증(everyone deny VIEW, verified allow VIEW+SEND)
- [ ] 의존 `desired-compiler → desired-state → discord-model`, 주석 없음
- [ ] Task별 커밋
