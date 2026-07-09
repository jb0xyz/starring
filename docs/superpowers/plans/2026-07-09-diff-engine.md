# Diff Engine Implementation Plan (Phase 4)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task가 크니 Task 안에서 여러 TDD 사이클/커밋을 돌리고 Task 끝에 보고.

**Goal:** `crates/diff-engine` — `diff(&NormalizedDesiredState, &resolver) -> DiffResult`. 현재 vs 목표 비교로 create/update/delete/no-op/conflict 계산.

**Architecture:** Resolver 트레이트로 정체성 해소 분리. Role/Channel/Overwrite diff. patch + 명시적 absent. 순수 Rust.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), discord-model·desired-state·desired-compiler(path deps).

## Global Constraints
> ⚠️ **주석 금지**: `//`, `///`, `//!` 없음. 코드 블록의 설명 문구 제거하고 구현.
- 의존: `diff-engine → {desired-compiler, desired-state, discord-model}`. 역방향 금지.
- `ResolveResult<T>`는 owned clone(lifetime 회피). `InMemoryMatchResolver`는 `&GuildState` 보유.
- 결정적 출력. 완료 게이트: `cargo build`/`test`/`clippy --all-targets -- -D warnings`/`fmt --all -- --check`.
- Task별 커밋, Task 끝에 보고.

---

### Task 1: 선행(MatchStrategy) + 스캐폴드 + Resolver

**Files:**
- Modify: `crates/desired-state/src/identity.rs` (MatchStrategy `#[non_exhaustive]` 제거)
- Modify: `Cargo.toml` (member 추가)
- Create: `crates/diff-engine/Cargo.toml`, `src/lib.rs`, `src/resolver.rs`

**Interfaces:**
- Produces: `ResolveResult<T>`(Existing/Missing/Conflict), `ResourceResolver`(resolve_role/resolve_channel/everyone_overwrite_target), `InMemoryMatchResolver`.

- [ ] **Step 1: MatchStrategy non_exhaustive 제거**

`crates/desired-state/src/identity.rs`의 `MatchStrategy` 정의에서 `#[non_exhaustive]` 줄을 제거. 결과:
```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", content = "value", rename_all = "snake_case")]
pub enum MatchStrategy {
    #[default]
    ByName,
    ByExplicitId(String),
}
```
Run: `cargo test -p desired-state` → 기존 테스트 통과 확인.

- [ ] **Step 2: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/diff-engine"` 추가.

Create `crates/diff-engine/Cargo.toml`:
```toml
[package]
name = "diff-engine"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
discord-model = { path = "../discord-model" }
desired-state = { path = "../desired-state" }
desired-compiler = { path = "../desired-compiler" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/diff-engine/src/lib.rs`:
```rust
pub mod resolver;

pub use resolver::{InMemoryMatchResolver, ResolveResult, ResourceResolver};
```

- [ ] **Step 3: Resolver 테스트 작성**

Create `crates/diff-engine/src/resolver.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::{Identity, MatchStrategy, ResourceKey};
    use discord_model::{Guild, GuildId, GuildState, Permissions, Role, RoleId, UserId};

    fn guild_with_roles(roles: Vec<Role>) -> GuildState {
        GuildState {
            guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) },
            roles,
            channels: vec![],
            members: vec![],
        }
    }

    fn role(id: u64, name: &str) -> Role {
        Role { id: RoleId(id), name: name.to_string(), permissions: Permissions::empty(), position: 0, managed: false }
    }

    fn ident(key: &str, m: MatchStrategy) -> Identity {
        Identity { key: ResourceKey(key.to_string()), match_by: m, ..Default::default() }
    }

    #[test]
    fn by_name_zero_one_two() {
        let g = guild_with_roles(vec![role(10, "a"), role(11, "b"), role(12, "b")]);
        let r = InMemoryMatchResolver::new(&g);
        assert!(matches!(r.resolve_role(&ident("k", MatchStrategy::ByName), Some("a")), ResolveResult::Existing(_)));
        assert!(matches!(r.resolve_role(&ident("k", MatchStrategy::ByName), Some("missing")), ResolveResult::Missing));
        assert!(matches!(r.resolve_role(&ident("k", MatchStrategy::ByName), Some("b")), ResolveResult::Conflict { .. }));
    }

    #[test]
    fn by_explicit_id() {
        let g = guild_with_roles(vec![role(10, "a")]);
        let r = InMemoryMatchResolver::new(&g);
        assert!(matches!(r.resolve_role(&ident("k", MatchStrategy::ByExplicitId("10".to_string())), None), ResolveResult::Existing(_)));
        assert!(matches!(r.resolve_role(&ident("k", MatchStrategy::ByExplicitId("99".to_string())), None), ResolveResult::Missing));
    }

    #[test]
    fn everyone_target_uses_guild_id() {
        use discord_model::OverwriteTarget;
        let g = guild_with_roles(vec![]);
        let r = InMemoryMatchResolver::new(&g);
        assert_eq!(r.everyone_overwrite_target(), OverwriteTarget::Role(RoleId(1)));
    }
}
```

- [ ] **Step 4: 실패 확인** — `cargo test -p diff-engine` → FAIL.

- [ ] **Step 5: Resolver 구현**

`resolver.rs` 테스트 위에:
```rust
use desired_state::{Identity, MatchStrategy};
use discord_model::{Channel, GuildState, OverwriteTarget, Role, RoleId};

pub enum ResolveResult<T> {
    Existing(T),
    Missing,
    Conflict { reason: String },
}

pub trait ResourceResolver {
    fn resolve_role(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Role>;
    fn resolve_channel(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Channel>;
    fn everyone_overwrite_target(&self) -> OverwriteTarget;
}

pub struct InMemoryMatchResolver<'a> {
    guild: &'a GuildState,
}

impl<'a> InMemoryMatchResolver<'a> {
    pub fn new(guild: &'a GuildState) -> Self {
        Self { guild }
    }
}

impl ResourceResolver for InMemoryMatchResolver<'_> {
    fn resolve_role(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Role> {
        match &identity.match_by {
            MatchStrategy::ByName => {
                let name = match name {
                    Some(n) => n,
                    None => return ResolveResult::Conflict { reason: "ByName requires a name".to_string() },
                };
                let matches: Vec<Role> = self.guild.roles.iter().filter(|r| r.name == name).cloned().collect();
                match matches.len() {
                    0 => ResolveResult::Missing,
                    1 => ResolveResult::Existing(matches.into_iter().next().unwrap()),
                    _ => ResolveResult::Conflict { reason: format!("multiple roles named {name}") },
                }
            }
            MatchStrategy::ByExplicitId(id) => match id.parse::<u64>() {
                Err(_) => ResolveResult::Conflict { reason: format!("invalid id {id}") },
                Ok(raw) => match self.guild.roles.iter().find(|r| r.id == RoleId(raw)).cloned() {
                    Some(r) => ResolveResult::Existing(r),
                    None => ResolveResult::Missing,
                },
            },
        }
    }

    fn resolve_channel(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Channel> {
        match &identity.match_by {
            MatchStrategy::ByName => {
                let name = match name {
                    Some(n) => n,
                    None => return ResolveResult::Conflict { reason: "ByName requires a name".to_string() },
                };
                let matches: Vec<Channel> = self.guild.channels.iter().filter(|c| c.name == name).cloned().collect();
                match matches.len() {
                    0 => ResolveResult::Missing,
                    1 => ResolveResult::Existing(matches.into_iter().next().unwrap()),
                    _ => ResolveResult::Conflict { reason: format!("multiple channels named {name}") },
                }
            }
            MatchStrategy::ByExplicitId(id) => match id.parse::<u64>() {
                Err(_) => ResolveResult::Conflict { reason: format!("invalid id {id}") },
                Ok(raw) => match self.guild.channels.iter().find(|c| c.id.0 == raw).cloned() {
                    Some(c) => ResolveResult::Existing(c),
                    None => ResolveResult::Missing,
                },
            },
        }
    }

    fn everyone_overwrite_target(&self) -> OverwriteTarget {
        OverwriteTarget::Role(RoleId(self.guild.guild.id.0))
    }
}
```

- [ ] **Step 6: 통과 + 커밋**
```bash
cargo test -p diff-engine && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(diff-engine): add ResourceResolver and InMemoryMatchResolver"
```

- [ ] **Step 7: Task 보고**

---

### Task 2: DiffResult 타입 + role diff

**Files:**
- Create: `crates/diff-engine/src/result.rs`, `crates/diff-engine/src/diff.rs`
- Modify: `crates/diff-engine/src/lib.rs`

**Interfaces:**
- Produces: `DiffResult/DiffChange/ChangeOp/DiffTarget/ChangedField/DiffConflict/DeferredItem`, `diff()`(role + panel deferred).

- [ ] **Step 1: 타입 + role diff 테스트**

Create `crates/diff-engine/src/result.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::ResourceKey;

    #[test]
    fn diff_result_roundtrip() {
        let mut d = DiffResult::default();
        d.changes.push(DiffChange {
            op: ChangeOp::Create,
            target: DiffTarget::Role { key: ResourceKey("r".to_string()) },
            changed: vec![],
        });
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<DiffResult>(&json).unwrap(), d);
    }
}
```

Create `crates/diff-engine/src/diff.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::InMemoryMatchResolver;
    use desired_compiler::{NormalizedDesiredState, NormalizedRole};
    use desired_state::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::{Guild, GuildId, GuildState, Permissions, Role, RoleId, UserId};

    fn empty_guild() -> GuildState {
        GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles: vec![], channels: vec![], members: vec![] }
    }

    fn guild_with(roles: Vec<Role>) -> GuildState {
        GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles, channels: vec![], members: vec![] }
    }

    fn nrole(key: &str, name: Option<&str>, ownership: Ownership, state: ResourceState) -> NormalizedRole {
        NormalizedRole {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                match_by: MatchStrategy::ByName,
                ownership,
                state,
            },
            name: name.map(|s| s.to_string()),
            permissions: None,
        }
    }

    fn ds(roles: Vec<NormalizedRole>) -> NormalizedDesiredState {
        NormalizedDesiredState { roles, ..Default::default() }
    }

    fn ops(d: &DiffResult) -> Vec<ChangeOp> {
        d.changes.iter().map(|c| c.op).collect()
    }

    #[test]
    fn present_missing_creates() {
        let g = empty_guild();
        let d = diff(&ds(vec![nrole("r", Some("New"), Ownership::Managed, ResourceState::Present)]), &InMemoryMatchResolver::new(&g));
        assert_eq!(ops(&d), vec![ChangeOp::Create]);
    }

    #[test]
    fn present_existing_same_is_noop() {
        let g = guild_with(vec![Role { id: RoleId(5), name: "Keep".to_string(), permissions: Permissions::empty(), position: 0, managed: false }]);
        let d = diff(&ds(vec![nrole("r", Some("Keep"), Ownership::Managed, ResourceState::Present)]), &InMemoryMatchResolver::new(&g));
        assert_eq!(ops(&d), vec![ChangeOp::NoOp]);
    }

    #[test]
    fn present_existing_diff_name_is_update() {
        let g = guild_with(vec![Role { id: RoleId(5), name: "Old".to_string(), permissions: Permissions::empty(), position: 0, managed: false }]);
        let mut nr = nrole("r", Some("Old"), Ownership::Managed, ResourceState::Present);
        nr.identity.match_by = MatchStrategy::ByExplicitId("5".to_string());
        nr.name = Some("Renamed".to_string());
        let d = diff(&ds(vec![nr]), &InMemoryMatchResolver::new(&g));
        assert_eq!(d.changes[0].op, ChangeOp::Update);
        assert_eq!(d.changes[0].changed, vec![ChangedField::Name]);
    }

    #[test]
    fn absent_existing_deletes() {
        let g = guild_with(vec![Role { id: RoleId(5), name: "Gone".to_string(), permissions: Permissions::empty(), position: 0, managed: false }]);
        let d = diff(&ds(vec![nrole("r", Some("Gone"), Ownership::Managed, ResourceState::Absent)]), &InMemoryMatchResolver::new(&g));
        assert_eq!(ops(&d), vec![ChangeOp::Delete]);
    }

    #[test]
    fn explicit_id_missing_is_conflict() {
        let g = empty_guild();
        let mut nr = nrole("r", None, Ownership::Managed, ResourceState::Present);
        nr.identity.match_by = MatchStrategy::ByExplicitId("99".to_string());
        let d = diff(&ds(vec![nr]), &InMemoryMatchResolver::new(&g));
        assert!(d.changes.is_empty());
        assert_eq!(d.conflicts.len(), 1);
    }

    #[test]
    fn referenced_missing_is_conflict() {
        let g = empty_guild();
        let d = diff(&ds(vec![nrole("r", Some("X"), Ownership::Referenced, ResourceState::Present)]), &InMemoryMatchResolver::new(&g));
        assert_eq!(d.conflicts.len(), 1);
    }
}
```

Modify `lib.rs`:
```rust
pub mod diff;
pub mod resolver;
pub mod result;

pub use diff::diff;
pub use resolver::{InMemoryMatchResolver, ResolveResult, ResourceResolver};
pub use result::{
    ChangeOp, ChangedField, DeferredItem, DiffChange, DiffConflict, DiffResult, DiffTarget,
};
```

- [ ] **Step 2: 실패 확인** — `cargo test -p diff-engine` → FAIL.

- [ ] **Step 3: result.rs 구현**

`result.rs` 테스트 위에:
```rust
use desired_compiler::NormalizedTarget;
use desired_state::ResourceKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<DiffChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<DiffConflict>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred: Vec<DeferredItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffChange {
    pub op: ChangeOp,
    pub target: DiffTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Create,
    Update,
    Delete,
    NoOp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffTarget {
    Role { key: ResourceKey },
    Channel { key: ResourceKey },
    Overwrite { channel: ResourceKey, target: NormalizedTarget },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangedField {
    Name,
    Permissions,
    ChannelType,
    Parent,
    Allow,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffConflict {
    pub target: DiffTarget,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferredItem {
    pub kind: String,
    pub key: ResourceKey,
    pub reason: String,
}
```

- [ ] **Step 4: diff.rs 구현 (role + panel deferred)**

`diff.rs` 테스트 위에:
```rust
use desired_compiler::{NormalizedDesiredState, NormalizedRole};
use desired_state::{Identity, MatchStrategy, Ownership, ResourceState};
use discord_model::Role;

use crate::resolver::{ResolveResult, ResourceResolver};
use crate::result::{ChangeOp, ChangedField, DeferredItem, DiffChange, DiffConflict, DiffResult, DiffTarget};

pub fn diff(desired: &NormalizedDesiredState, resolver: &impl ResourceResolver) -> DiffResult {
    let mut out = DiffResult::default();
    for role in &desired.roles {
        diff_role(role, resolver, &mut out);
    }
    for panel in &desired.verification_panels {
        out.deferred.push(DeferredItem {
            kind: "verification_panel".to_string(),
            key: panel.identity.key.clone(),
            reason: "panel state not tracked in Phase 4".to_string(),
        });
    }
    out
}

fn is_explicit_id(id: &Identity) -> bool {
    matches!(id.match_by, MatchStrategy::ByExplicitId(_))
}

fn noop(target: DiffTarget) -> DiffChange {
    DiffChange { op: ChangeOp::NoOp, target, changed: vec![] }
}

fn push_conflict(out: &mut DiffResult, target: DiffTarget, reason: &str) {
    out.conflicts.push(DiffConflict { target, reason: reason.to_string() });
}

fn diff_role(role: &NormalizedRole, resolver: &impl ResourceResolver, out: &mut DiffResult) {
    let id = &role.identity;
    let target = DiffTarget::Role { key: id.key.clone() };
    let resolved = resolver.resolve_role(id, role.name.as_deref());
    match id.state {
        ResourceState::Present => match resolved {
            ResolveResult::Existing(current) => {
                if id.ownership == Ownership::Referenced {
                    out.changes.push(noop(target));
                } else {
                    let changed = role_changed(role, &current);
                    if changed.is_empty() {
                        out.changes.push(noop(target));
                    } else {
                        out.changes.push(DiffChange { op: ChangeOp::Update, target, changed });
                    }
                }
            }
            ResolveResult::Missing => {
                if id.ownership == Ownership::Referenced {
                    push_conflict(out, target, "referenced role not found");
                } else if is_explicit_id(id) {
                    push_conflict(out, target, "explicit id not found");
                } else {
                    out.changes.push(DiffChange { op: ChangeOp::Create, target, changed: vec![] });
                }
            }
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
        ResourceState::Absent => match resolved {
            ResolveResult::Existing(_) => {
                if id.ownership == Ownership::Referenced {
                    push_conflict(out, target, "cannot delete referenced role");
                } else {
                    out.changes.push(DiffChange { op: ChangeOp::Delete, target, changed: vec![] });
                }
            }
            ResolveResult::Missing => out.changes.push(noop(target)),
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
    }
}

fn role_changed(role: &NormalizedRole, current: &Role) -> Vec<ChangedField> {
    let mut changed = Vec::new();
    if let Some(name) = &role.name {
        if name != &current.name {
            changed.push(ChangedField::Name);
        }
    }
    if let Some(perms) = &role.permissions {
        if perms != &current.permissions {
            changed.push(ChangedField::Permissions);
        }
    }
    changed
}
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p diff-engine && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(diff-engine): add DiffResult types and role diff"
```

- [ ] **Step 6: Task 보고**

---

### Task 3: channel diff + overwrite diff

**Files:**
- Modify: `crates/diff-engine/src/diff.rs`

**Interfaces:**
- Produces: `diff()`에 channel(메타) + overwrite(target 해소·가산) 추가.

- [ ] **Step 1: channel/overwrite 테스트 추가**

`diff.rs` 테스트 모듈에 추가:
```rust
    use desired_compiler::{NormalizedChannel, NormalizedOverwrite, NormalizedTarget};
    use discord_model::{Channel, ChannelId, ChannelType, OverwriteTarget, PermissionOverwrite};

    fn nchannel(key: &str, name: &str, overwrites: Vec<NormalizedOverwrite>) -> NormalizedChannel {
        NormalizedChannel {
            identity: Identity { key: ResourceKey(key.to_string()), match_by: MatchStrategy::ByName, ..Default::default() },
            name: Some(name.to_string()),
            channel_type: None,
            parent: None,
            overwrites,
        }
    }

    fn guild_full(roles: Vec<Role>, channels: Vec<Channel>) -> GuildState {
        GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles, channels, members: vec![] }
    }

    #[test]
    fn everyone_overwrite_noop_when_matching() {
        let cur_channel = Channel {
            id: ChannelId(20),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            }],
        };
        let g = guild_full(vec![], vec![cur_channel]);
        let desired = NormalizedDesiredState {
            channels: vec![nchannel("gen", "general", vec![NormalizedOverwrite {
                target: NormalizedTarget::Everyone,
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            }])],
            ..Default::default()
        };
        let d = diff(&desired, &InMemoryMatchResolver::new(&g));
        assert!(d.conflicts.is_empty());
        assert!(d.changes.iter().all(|c| c.op == ChangeOp::NoOp));
    }

    #[test]
    fn role_overwrite_created_when_absent_in_current() {
        let verified = Role { id: RoleId(50), name: "Verified".to_string(), permissions: Permissions::empty(), position: 0, managed: false };
        let cur_channel = Channel {
            id: ChannelId(20), name: "general".to_string(), channel_type: ChannelType::Text,
            parent_id: None, position: 0, overwrites: vec![],
        };
        let g = guild_full(vec![verified], vec![cur_channel]);
        let desired = NormalizedDesiredState {
            roles: vec![nrole("vk", Some("Verified"), Ownership::Managed, ResourceState::Present)],
            channels: vec![nchannel("gen", "general", vec![NormalizedOverwrite {
                target: NormalizedTarget::Role(ResourceKey("vk".to_string())),
                allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                deny: Permissions::empty(),
            }])],
            ..Default::default()
        };
        let d = diff(&desired, &InMemoryMatchResolver::new(&g));
        assert!(d.conflicts.is_empty());
        let ow = d.changes.iter().find(|c| matches!(c.target, DiffTarget::Overwrite { .. })).unwrap();
        assert_eq!(ow.op, ChangeOp::Create);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test -p diff-engine` → FAIL.

- [ ] **Step 3: diff.rs에 channel/overwrite 구현**

`diff()` 함수의 role 루프 뒤, panel 루프 앞에 추가:
```rust
    let roles_by_key: std::collections::HashMap<&desired_state::ResourceKey, &NormalizedRole> =
        desired.roles.iter().map(|r| (&r.identity.key, r)).collect();
    for channel in &desired.channels {
        diff_channel(channel, resolver, &roles_by_key, &mut out);
    }
```

파일 상단 use에 추가:
```rust
use std::collections::HashMap;

use desired_compiler::{NormalizedChannel, NormalizedOverwrite, NormalizedTarget};
use desired_state::ResourceKey;
use discord_model::{Channel, OverwriteTarget};
```

그리고 함수 추가:
```rust
fn diff_channel(
    channel: &NormalizedChannel,
    resolver: &impl ResourceResolver,
    roles_by_key: &HashMap<&ResourceKey, &NormalizedRole>,
    out: &mut DiffResult,
) {
    let id = &channel.identity;
    let target = DiffTarget::Channel { key: id.key.clone() };
    let resolved = resolver.resolve_channel(id, channel.name.as_deref());
    match id.state {
        ResourceState::Present => match resolved {
            ResolveResult::Existing(current) => {
                if id.ownership == Ownership::Referenced {
                    out.changes.push(noop(target));
                } else {
                    let changed = channel_meta_changed(channel, &current);
                    if changed.is_empty() {
                        out.changes.push(noop(target));
                    } else {
                        out.changes.push(DiffChange { op: ChangeOp::Update, target, changed });
                    }
                    diff_overwrites(channel, &current, resolver, roles_by_key, out);
                }
            }
            ResolveResult::Missing => {
                if id.ownership == Ownership::Referenced {
                    push_conflict(out, target, "referenced channel not found");
                } else if is_explicit_id(id) {
                    push_conflict(out, target, "explicit id not found");
                } else {
                    out.changes.push(DiffChange { op: ChangeOp::Create, target, changed: vec![] });
                }
            }
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
        ResourceState::Absent => match resolved {
            ResolveResult::Existing(_) => {
                if id.ownership == Ownership::Referenced {
                    push_conflict(out, target, "cannot delete referenced channel");
                } else {
                    out.changes.push(DiffChange { op: ChangeOp::Delete, target, changed: vec![] });
                }
            }
            ResolveResult::Missing => out.changes.push(noop(target)),
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
    }
}

fn channel_meta_changed(channel: &NormalizedChannel, current: &Channel) -> Vec<ChangedField> {
    let mut changed = Vec::new();
    if let Some(name) = &channel.name {
        if name != &current.name {
            changed.push(ChangedField::Name);
        }
    }
    if let Some(ct) = &channel.channel_type {
        if ct != &current.channel_type {
            changed.push(ChangedField::ChannelType);
        }
    }
    changed
}

fn diff_overwrites(
    channel: &NormalizedChannel,
    current: &Channel,
    resolver: &impl ResourceResolver,
    roles_by_key: &HashMap<&ResourceKey, &NormalizedRole>,
    out: &mut DiffResult,
) {
    for ow in &channel.overwrites {
        let dt = DiffTarget::Overwrite { channel: channel.identity.key.clone(), target: ow.target.clone() };
        let current_target = match resolve_overwrite_target(&ow.target, resolver, roles_by_key) {
            Ok(t) => t,
            Err(reason) => {
                push_conflict(out, dt, &reason);
                continue;
            }
        };
        match current_target.and_then(|ct| current.overwrites.iter().find(|o| o.target == ct)) {
            Some(cur) => {
                let mut changed = Vec::new();
                if ow.allow != cur.allow {
                    changed.push(ChangedField::Allow);
                }
                if ow.deny != cur.deny {
                    changed.push(ChangedField::Deny);
                }
                if changed.is_empty() {
                    out.changes.push(noop(dt));
                } else {
                    out.changes.push(DiffChange { op: ChangeOp::Update, target: dt, changed });
                }
            }
            None => out.changes.push(DiffChange { op: ChangeOp::Create, target: dt, changed: vec![] }),
        }
    }
}

fn resolve_overwrite_target(
    target: &NormalizedTarget,
    resolver: &impl ResourceResolver,
    roles_by_key: &HashMap<&ResourceKey, &NormalizedRole>,
) -> Result<Option<OverwriteTarget>, String> {
    match target {
        NormalizedTarget::Everyone => Ok(Some(resolver.everyone_overwrite_target())),
        NormalizedTarget::Member(id) => match id.parse::<u64>() {
            Ok(raw) => Ok(Some(OverwriteTarget::Member(discord_model::UserId(raw)))),
            Err(_) => Err(format!("invalid member id {id}")),
        },
        NormalizedTarget::Role(key) => match roles_by_key.get(key) {
            None => Err(format!("overwrite references undeclared role key {}", key.0)),
            Some(nr) => match resolver.resolve_role(&nr.identity, nr.name.as_deref()) {
                ResolveResult::Existing(r) => Ok(Some(OverwriteTarget::Role(r.id))),
                ResolveResult::Missing => Ok(None),
                ResolveResult::Conflict { reason } => Err(reason),
            },
        },
    }
}
```

> 참고: `current_target`이 `None`(역할이 아직 없음/신규)이면 현재 매칭 overwrite가 없으므로 Create가 된다. `Ok(None)` → `and_then`으로 `None` → Create 분기.

- [ ] **Step 4: 통과 + 커밋**
```bash
cargo test -p diff-engine && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(diff-engine): add channel and overwrite diff with target resolution"
```

- [ ] **Step 5: Task 보고**

---

### Task 4: 멱등성 통합 픽스처 + 최종 게이트

**Files:**
- Create: `crates/diff-engine/tests/verification_scenario.rs`

**Interfaces:**
- Produces: DesiredState → compile → diff. before→변경 / after→전부 NoOp(멱등성).

- [ ] **Step 1: 통합 테스트 작성**

Create `crates/diff-engine/tests/verification_scenario.rs`:
```rust
use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, FeatureIntent, Identity,
    ResourceKey, RoleIntent, VerificationIntent,
};
use diff_engine::{diff, ChangeOp, InMemoryMatchResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};

fn desired() -> DesiredState {
    let verified = ResourceKey("verified_member".to_string());
    let mut general_roles = BTreeMap::new();
    general_roles.insert(
        verified.clone(),
        AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: verified.clone(), ..Default::default() },
            name: Some("Verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity { key: ResourceKey("general".to_string()), ..Default::default() },
            name: Some("general".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
                roles: general_roles,
            }),
            raw_overwrites: None,
        }],
        features: vec![FeatureIntent::Verification(VerificationIntent {
            identity: Identity { key: ResourceKey("panel".to_string()), ..Default::default() },
            channel: ResourceKey("general".to_string()),
            grants_role: verified,
        })],
        ..Default::default()
    }
}

fn after_guild() -> GuildState {
    GuildState {
        guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) },
        roles: vec![Role { id: RoleId(50), name: "Verified".to_string(), permissions: Permissions::empty(), position: 0, managed: false }],
        channels: vec![Channel {
            id: ChannelId(20),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![
                PermissionOverwrite { target: OverwriteTarget::Role(RoleId(1)), allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL },
                PermissionOverwrite { target: OverwriteTarget::Role(RoleId(50)), allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES, deny: Permissions::empty() },
            ],
        }],
        members: vec![],
    }
}

#[test]
fn diff_on_empty_guild_creates() {
    let normalized = compile(&desired()).unwrap();
    let empty = GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles: vec![], channels: vec![], members: vec![] };
    let d = diff(&normalized, &InMemoryMatchResolver::new(&empty));
    assert!(d.changes.iter().any(|c| c.op == ChangeOp::Create));
    assert_eq!(d.deferred.len(), 1);
}

#[test]
fn diff_on_matching_guild_is_all_noop() {
    let normalized = compile(&desired()).unwrap();
    let guild = after_guild();
    let d = diff(&normalized, &InMemoryMatchResolver::new(&guild));
    assert!(d.conflicts.is_empty(), "conflicts: {:?}", d.conflicts);
    assert!(d.changes.iter().all(|c| c.op == ChangeOp::NoOp), "changes: {:?}", d.changes);
    assert_eq!(d.deferred.len(), 1);
}
```

> `ChannelId`는 discord_model에서 import 필요 — 위 use 블록에 빠져 있으면 추가하라. `after_guild`의 @everyone overwrite는 `Role(RoleId(1))`(=guild id 1), verified는 `Role(RoleId(50))`로 desired 산출과 정확히 일치해야 NoOp이 나온다.

- [ ] **Step 2: 통과 확인** — `cargo test -p diff-engine --test verification_scenario`. 실패 시 fixture의 overwrite 값/타깃이 compile 산출과 일치하는지 대조(특히 @everyone=Role(RoleId(1)), verified=Role(RoleId(50))).

- [ ] **Step 3: 최종 품질 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트는 실제 출력대로 보고(diff-engine 단위 + 통합 2 + 기존 phase).

- [ ] **Step 4: 커밋 + 보고**
```bash
git add -A
git commit -m "test(diff-engine): add verification scenario idempotency fixtures"
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 전부 통과
- [ ] desired-state: MatchStrategy non_exhaustive 제거
- [ ] diff-engine: ResourceResolver+InMemoryMatchResolver, DiffResult 타입, role/channel/overwrite diff, present/absent/ownership/target 해소, panel deferred
- [ ] **멱등성**: after_guild diff → 전부 NoOp, conflict 0, deferred 1
- [ ] before(empty) diff → Create 포함
- [ ] 의존 `diff-engine → {desired-compiler, desired-state, discord-model}`, 주석 없음
- [ ] Task별 커밋
