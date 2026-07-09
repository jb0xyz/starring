# Resource Resolution 추출 Implementation Plan (Phase 12a)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `virtual-apply`의 key/id 해소 로직을 `crates/resource-resolution` 공용 crate로 추출하고, `virtual-apply`가 이를 사용하도록 리팩토링. **동작 변화 0** — Virtual Apply와 (미래) Executor가 같은 해소 코드를 공유해 preview-exec drift를 방지.

**Architecture:** `ResourceResolutionContext`(bindings + normalized + resolver + guild_id)가 3단계 해소(binding→resolver→error)를 담당. 레이어는 id를 mint하지 않음 — 호출자가 mint 후 `bind`. virtual-apply는 synthetic 카운터·after-state 변형만 유지.

**Tech Stack:** Rust edition 2021 stable, thiserror, 기존 코어 crate.

## Global Constraints
> ⚠️ **주석 금지**. **순수 리팩토링 — 기존 126 테스트 전부 그대로 통과해야 함**(테스트 변경 금지).
- 의존: `resource-resolution → {desired-compiler, desired-state, diff-engine, discord-model}`. `virtual-apply → + resource-resolution`.
- 스펙: `docs/superpowers/specs/2026-07-09-executor-bot-runtime-design.md` §2/§3/§7.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋. **Phase 완료 후 `git push origin main`.**

---

### Task 1: resource-resolution crate 신설

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/resource-resolution/Cargo.toml`, `src/{lib.rs, error.rs, bindings.rs, context.rs}`

**Interfaces:**
- Produces: `ResolutionError`, `ResourceBindingMap`, `ResourceResolutionContext` + `new/bind_role/bind_channel/resolve_role_key/resolve_channel_key/resolve_target`.

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/resource-resolution"` 추가.

Create `crates/resource-resolution/Cargo.toml`:
```toml
[package]
name = "resource-resolution"
version = "0.1.0"
edition.workspace = true

[dependencies]
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }
desired-state = { path = "../desired-state" }
desired-compiler = { path = "../desired-compiler" }
diff-engine = { path = "../diff-engine" }
```

Create `crates/resource-resolution/src/lib.rs`:
```rust
pub mod bindings;
pub mod context;
pub mod error;

pub use bindings::ResourceBindingMap;
pub use context::ResourceResolutionContext;
pub use error::ResolutionError;
```

Create `crates/resource-resolution/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ResolutionError {
    #[error("unresolved key: {key}")]
    UnresolvedKey { key: String },
    #[error("missing identity for key: {key}")]
    MissingIdentity { key: String },
}
```

Create `crates/resource-resolution/src/bindings.rs`:
```rust
use std::collections::BTreeMap;

use desired_state::ResourceKey;
use discord_model::{ChannelId, RoleId};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceBindingMap {
    pub role_bindings: BTreeMap<ResourceKey, RoleId>,
    pub channel_bindings: BTreeMap<ResourceKey, ChannelId>,
}
```

- [ ] **Step 2: context.rs 테스트 작성**

Create `crates/resource-resolution/src/context.rs` (테스트 먼저):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_compiler::NormalizedRole;
    use desired_state::Identity;
    use diff_engine::InMemoryMatchResolver;
    use discord_model::{Guild, GuildState, Permissions, Role};

    fn empty_guild(guild_id: u64) -> GuildState {
        GuildState {
            guild: Guild { id: GuildId(guild_id), name: "g".to_string(), owner_id: UserId(1) },
            roles: vec![],
            channels: vec![],
            members: vec![],
        }
    }
    fn nrole(key: &str, name: &str) -> NormalizedRole {
        NormalizedRole {
            identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() },
            name: Some(name.to_string()),
            permissions: Some(Permissions::empty()),
        }
    }

    #[test]
    fn bound_role_resolves_to_binding() {
        let normalized = NormalizedDesiredState::default();
        let guild = empty_guild(1);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        ctx.bind_role(ResourceKey("verified".to_string()), RoleId(999));
        assert_eq!(
            ctx.resolve_role_key(&ResourceKey("verified".to_string())).unwrap(),
            RoleId(999)
        );
    }

    #[test]
    fn existing_role_resolves_via_resolver() {
        let normalized = NormalizedDesiredState { roles: vec![nrole("mod", "Moderator")], ..Default::default() };
        let guild = GuildState {
            guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) },
            roles: vec![Role { id: RoleId(42), name: "Moderator".to_string(), permissions: Permissions::empty(), position: 0, managed: false }],
            channels: vec![],
            members: vec![],
        };
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        assert_eq!(ctx.resolve_role_key(&ResourceKey("mod".to_string())).unwrap(), RoleId(42));
    }

    #[test]
    fn missing_role_errors_unresolved() {
        let normalized = NormalizedDesiredState { roles: vec![nrole("ghost", "Ghost")], ..Default::default() };
        let guild = empty_guild(1);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        assert!(matches!(
            ctx.resolve_role_key(&ResourceKey("ghost".to_string())),
            Err(ResolutionError::UnresolvedKey { .. })
        ));
    }

    #[test]
    fn unknown_key_errors_missing_identity() {
        let normalized = NormalizedDesiredState::default();
        let guild = empty_guild(1);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        assert!(matches!(
            ctx.resolve_role_key(&ResourceKey("nope".to_string())),
            Err(ResolutionError::MissingIdentity { .. })
        ));
    }

    #[test]
    fn resolve_target_everyone_uses_guild_id() {
        let normalized = NormalizedDesiredState::default();
        let guild = empty_guild(7);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(7));
        assert_eq!(
            ctx.resolve_target(&NormalizedTarget::Everyone).unwrap(),
            OverwriteTarget::Role(RoleId(7))
        );
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p resource-resolution` → FAIL(context 미구현).

- [ ] **Step 4: context.rs 구현**

`context.rs` 테스트 위에:
```rust
use desired_compiler::{NormalizedDesiredState, NormalizedTarget};
use desired_state::ResourceKey;
use diff_engine::{ResolveResult, ResourceResolver};
use discord_model::{ChannelId, GuildId, OverwriteTarget, RoleId, UserId};

use crate::bindings::ResourceBindingMap;
use crate::error::ResolutionError;

pub struct ResourceResolutionContext<'a, R: ResourceResolver> {
    pub bindings: ResourceBindingMap,
    normalized: &'a NormalizedDesiredState,
    resolver: &'a R,
    guild_id: GuildId,
}

impl<'a, R: ResourceResolver> ResourceResolutionContext<'a, R> {
    pub fn new(normalized: &'a NormalizedDesiredState, resolver: &'a R, guild_id: GuildId) -> Self {
        Self {
            bindings: ResourceBindingMap::default(),
            normalized,
            resolver,
            guild_id,
        }
    }

    pub fn bind_role(&mut self, key: ResourceKey, id: RoleId) {
        self.bindings.role_bindings.insert(key, id);
    }

    pub fn bind_channel(&mut self, key: ResourceKey, id: ChannelId) {
        self.bindings.channel_bindings.insert(key, id);
    }

    pub fn resolve_role_key(&mut self, key: &ResourceKey) -> Result<RoleId, ResolutionError> {
        if let Some(id) = self.bindings.role_bindings.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nr = self
                .normalized
                .roles
                .iter()
                .find(|r| &r.identity.key == key)
                .ok_or_else(|| ResolutionError::MissingIdentity { key: key.0.clone() })?;
            self.resolver.resolve_role(&nr.identity, nr.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(role) => {
                self.bindings.role_bindings.insert(key.clone(), role.id);
                Ok(role.id)
            }
            _ => Err(ResolutionError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    pub fn resolve_channel_key(&mut self, key: &ResourceKey) -> Result<ChannelId, ResolutionError> {
        if let Some(id) = self.bindings.channel_bindings.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nc = self
                .normalized
                .channels
                .iter()
                .find(|c| &c.identity.key == key)
                .ok_or_else(|| ResolutionError::MissingIdentity { key: key.0.clone() })?;
            self.resolver.resolve_channel(&nc.identity, nc.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(ch) => {
                self.bindings.channel_bindings.insert(key.clone(), ch.id);
                Ok(ch.id)
            }
            _ => Err(ResolutionError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    pub fn resolve_target(
        &mut self,
        target: &NormalizedTarget,
    ) -> Result<OverwriteTarget, ResolutionError> {
        match target {
            NormalizedTarget::Everyone => Ok(OverwriteTarget::Role(RoleId(self.guild_id.0))),
            NormalizedTarget::Role(key) => Ok(OverwriteTarget::Role(self.resolve_role_key(key)?)),
            NormalizedTarget::Member(id) => {
                let raw = id
                    .parse::<u64>()
                    .map_err(|_| ResolutionError::UnresolvedKey { key: id.clone() })?;
                Ok(OverwriteTarget::Member(UserId(raw)))
            }
        }
    }
}
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p resource-resolution && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(resource-resolution): extract shared key/id resolution layer"
```

- [ ] **Step 6: Task 보고**

---

### Task 2: virtual-apply 리팩토링 (resource-resolution 사용)

**Files:**
- Modify: `crates/virtual-apply/Cargo.toml`, `src/error.rs`, `src/apply.rs`

**Interfaces:**
- Consumes: `resource_resolution::{ResourceResolutionContext, ResolutionError}`.
- 공개 API 불변: `apply(...)`, `VirtualApplyError`, `VirtualApplyResult` 그대로.

- [ ] **Step 1: Cargo dep 추가**

`crates/virtual-apply/Cargo.toml` [dependencies]에 추가:
```toml
resource-resolution = { path = "../resource-resolution" }
```

- [ ] **Step 2: error.rs — From 변환 추가**

`crates/virtual-apply/src/error.rs` 전체 교체:
```rust
use resource_resolution::ResolutionError;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum VirtualApplyError {
    #[error("unresolved key: {key}")]
    UnresolvedKey { key: String },
    #[error("missing identity for key: {key}")]
    MissingIdentity { key: String },
    #[error("operation graph cycle")]
    GraphCycle,
}

impl From<ResolutionError> for VirtualApplyError {
    fn from(err: ResolutionError) -> Self {
        match err {
            ResolutionError::UnresolvedKey { key } => VirtualApplyError::UnresolvedKey { key },
            ResolutionError::MissingIdentity { key } => VirtualApplyError::MissingIdentity { key },
        }
    }
}
```

- [ ] **Step 3: apply.rs — 해소 로직을 context로 위임**

`crates/virtual-apply/src/apply.rs`의 **production 코드**(`#[cfg(test)]` 위 전부)를 아래로 교체. **테스트 모듈(`#[cfg(test)] mod tests`)은 절대 변경하지 말 것** — 그게 동작 보존의 증거.
```rust
use std::collections::BTreeMap;

use desired_compiler::NormalizedDesiredState;
use desired_state::ResourceKey;
use diff_engine::ResourceResolver;
use discord_model::{
    Channel, ChannelId, ChannelType, GuildState, PermissionOverwrite, Role, RoleId,
};
use operation_graph::{OpId, Operation, OperationGraph};
use resource_resolution::ResourceResolutionContext;

use crate::error::VirtualApplyError;
use crate::result::VirtualApplyResult;

pub fn apply(
    current: &GuildState,
    graph: &OperationGraph,
    normalized: &NormalizedDesiredState,
    resolver: &impl ResourceResolver,
) -> Result<VirtualApplyResult, VirtualApplyError> {
    let mut ctx = ApplyContext::new(current, normalized, resolver);
    let order = graph
        .topological_order()
        .map_err(|_| VirtualApplyError::GraphCycle)?;
    for id in order {
        if let Some(node) = graph.nodes.iter().find(|n| n.id == id) {
            ctx.apply_operation(&node.operation)?;
            ctx.applied.push(id);
        }
    }
    Ok(ctx.into_result())
}

struct ApplyContext<'a, R: ResourceResolver> {
    after: GuildState,
    resources: ResourceResolutionContext<'a, R>,
    next_id: u64,
    synthetic_roles: BTreeMap<ResourceKey, RoleId>,
    synthetic_channels: BTreeMap<ResourceKey, ChannelId>,
    applied: Vec<OpId>,
    warnings: Vec<String>,
}

impl<'a, R: ResourceResolver> ApplyContext<'a, R> {
    fn new(current: &GuildState, normalized: &'a NormalizedDesiredState, resolver: &'a R) -> Self {
        let mut max_id = current.guild.id.0;
        for r in &current.roles {
            max_id = max_id.max(r.id.0);
        }
        for c in &current.channels {
            max_id = max_id.max(c.id.0);
        }
        Self {
            after: current.clone(),
            resources: ResourceResolutionContext::new(normalized, resolver, current.guild.id),
            next_id: max_id + 1,
            synthetic_roles: BTreeMap::new(),
            synthetic_channels: BTreeMap::new(),
            applied: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn next_synthetic(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn apply_operation(&mut self, op: &Operation) -> Result<(), VirtualApplyError> {
        match op {
            Operation::CreateRole {
                key,
                name,
                permissions,
            } => {
                let id = RoleId(self.next_synthetic());
                self.resources.bind_role(key.clone(), id);
                self.synthetic_roles.insert(key.clone(), id);
                self.after.roles.push(Role {
                    id,
                    name: name.clone().unwrap_or_default(),
                    permissions: permissions.unwrap_or_default(),
                    position: 0,
                    managed: false,
                });
            }
            Operation::UpdateRole {
                key,
                name,
                permissions,
            } => {
                let id = self.resources.resolve_role_key(key)?;
                if let Some(role) = self.after.roles.iter_mut().find(|r| r.id == id) {
                    if let Some(n) = name {
                        role.name = n.clone();
                    }
                    if let Some(p) = permissions {
                        role.permissions = *p;
                    }
                }
            }
            Operation::DeleteRole { key } => {
                let id = self.resources.resolve_role_key(key)?;
                self.after.roles.retain(|r| r.id != id);
            }
            Operation::CreateChannel {
                key,
                name,
                channel_type,
                parent,
            } => {
                let id = ChannelId(self.next_synthetic());
                self.resources.bind_channel(key.clone(), id);
                self.synthetic_channels.insert(key.clone(), id);
                let parent_id = match parent {
                    Some(pk) => Some(self.resources.resolve_channel_key(pk)?),
                    None => None,
                };
                self.after.channels.push(Channel {
                    id,
                    name: name.clone().unwrap_or_default(),
                    channel_type: channel_type.unwrap_or(ChannelType::Text),
                    parent_id,
                    position: 0,
                    overwrites: Vec::new(),
                });
            }
            Operation::UpdateChannel {
                key,
                name,
                channel_type,
            } => {
                let id = self.resources.resolve_channel_key(key)?;
                if let Some(ch) = self.after.channels.iter_mut().find(|c| c.id == id) {
                    if let Some(n) = name {
                        ch.name = n.clone();
                    }
                    if let Some(t) = channel_type {
                        ch.channel_type = *t;
                    }
                }
            }
            Operation::DeleteChannel { key } => {
                let id = self.resources.resolve_channel_key(key)?;
                self.after.channels.retain(|c| c.id != id);
            }
            Operation::CreateOverwrite {
                channel,
                target,
                allow,
                deny,
            }
            | Operation::UpdateOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let channel_id = self.resources.resolve_channel_key(channel)?;
                let ow_target = self.resources.resolve_target(target)?;
                if let Some(ch) = self.after.channels.iter_mut().find(|c| c.id == channel_id) {
                    ch.overwrites.retain(|o| o.target != ow_target);
                    ch.overwrites.push(PermissionOverwrite {
                        target: ow_target,
                        allow: *allow,
                        deny: *deny,
                    });
                } else {
                    self.warnings
                        .push(format!("overwrite channel not found: {}", channel.0));
                }
            }
        }
        Ok(())
    }

    fn into_result(self) -> VirtualApplyResult {
        VirtualApplyResult {
            after: self.after,
            applied: self.applied,
            synthetic_roles: self.synthetic_roles,
            synthetic_channels: self.synthetic_channels,
            warnings: self.warnings,
        }
    }
}
```

- [ ] **Step 4: 통과 확인 (동작 보존 증거)** — `cargo test -p virtual-apply`. 기존 4개(단위 3 + pipeline 1) **그대로 통과**해야 함. 실패하면 리팩토링이 동작을 바꾼 것 → 원인 수정.

- [ ] **Step 5: 최종 게이트 (전체 126 + 신규 유지)**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. **기존 126개 그대로 + resource-resolution 신규 5개** = 131 (실제 출력대로 보고).

- [ ] **Step 6: 커밋 + push + 보고**
```bash
git add -A
git commit -m "refactor(virtual-apply): use shared resource-resolution context"
git push origin main
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] resource-resolution: ResourceBindingMap/ResolutionError/ResourceResolutionContext(new/bind/resolve_role_key/resolve_channel_key/resolve_target)
- [ ] virtual-apply 공개 API 불변(apply/VirtualApplyError/VirtualApplyResult), 해소는 context에 위임, `From<ResolutionError>` 변환
- [ ] **기존 126 테스트 전부 그대로 통과**(virtual-apply 테스트 모듈 무변경 = 동작 보존 증거) + resource-resolution 신규 테스트
- [ ] 의존 방향·주석 없음·Task별 커밋·**main push**
