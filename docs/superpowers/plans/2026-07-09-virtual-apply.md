# Virtual Apply Engine Implementation Plan (Phase 9)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `crates/virtual-apply` — `apply(current, graph, normalized, resolver) -> Result<VirtualApplyResult, _>`. OperationGraph를 GuildState 복사본에 topo 순서로 가상 적용, synthetic id + Resolver 해소.

**Architecture:** ApplyContext가 after GuildState를 들고 key→id(synthetic/current) 맵으로 8 op를 적용. Discord API/DB 없음.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), thiserror, operation-graph·desired-compiler·desired-state·diff-engine·discord-model.

## Global Constraints
> ⚠️ **주석 금지**. 결정적(synthetic 카운터·topo 순서).
- 의존: `virtual-apply → {operation-graph, desired-compiler, desired-state, diff-engine, discord-model}`.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋, Task 끝에 보고. **Phase 완료 후 `git push origin main`.**

---

### Task 1: 스캐폴드 + 타입 + apply() 엔진

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/virtual-apply/Cargo.toml`, `src/{lib.rs, error.rs, result.rs, apply.rs}`

**Interfaces:**
- Produces: `VirtualApplyResult`, `VirtualApplyError`, `apply(...)`.

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/virtual-apply"` 추가.

Create `crates/virtual-apply/Cargo.toml`:
```toml
[package]
name = "virtual-apply"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }
operation-graph = { path = "../operation-graph" }
desired-compiler = { path = "../desired-compiler" }
desired-state = { path = "../desired-state" }
diff-engine = { path = "../diff-engine" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/virtual-apply/src/lib.rs`:
```rust
pub mod apply;
pub mod error;
pub mod result;

pub use apply::apply;
pub use error::VirtualApplyError;
pub use result::VirtualApplyResult;
```

Create `crates/virtual-apply/src/error.rs`:
```rust
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
```

Create `crates/virtual-apply/src/result.rs`:
```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildState, RoleId};
use operation_graph::OpId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualApplyResult {
    pub after: GuildState,
    pub applied: Vec<OpId>,
    pub synthetic_roles: BTreeMap<ResourceKey, RoleId>,
    pub synthetic_channels: BTreeMap<ResourceKey, ChannelId>,
    pub warnings: Vec<String>,
}
```

- [ ] **Step 2: apply 테스트 작성**

Create `crates/virtual-apply/src/apply.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_compiler::{NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole, NormalizedTarget};
    use desired_state::{Identity, ResourceKey};
    use diff_engine::InMemoryMatchResolver;
    use discord_model::{Guild, GuildId, GuildState, OverwriteTarget, Permissions, UserId};
    use operation_graph::{OpId, Operation, OperationGraph, OperationNode};

    fn empty_guild() -> GuildState {
        GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles: vec![], channels: vec![], members: vec![] }
    }
    fn nrole(key: &str, name: &str) -> NormalizedRole {
        NormalizedRole { identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() }, name: Some(name.to_string()), permissions: Some(Permissions::empty()) }
    }
    fn nchannel(key: &str, name: &str, overwrites: Vec<NormalizedOverwrite>) -> NormalizedChannel {
        NormalizedChannel { identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() }, name: Some(name.to_string()), channel_type: None, parent: None, overwrites }
    }
    fn node(id: u32, op: Operation) -> OperationNode {
        OperationNode { id: OpId(id), operation: op, produces: vec![], consumes: vec![], depends_on: vec![] }
    }

    #[test]
    fn create_role_gets_synthetic_id() {
        let normalized = NormalizedDesiredState { roles: vec![nrole("vip", "VIP")], ..Default::default() };
        let graph = OperationGraph { nodes: vec![node(0, Operation::CreateRole { key: ResourceKey("vip".to_string()), name: Some("VIP".to_string()), permissions: Some(Permissions::empty()) })] };
        let current = empty_guild();
        let resolver = InMemoryMatchResolver::new(&current);
        let result = apply(&current, &graph, &normalized, &resolver).unwrap();
        assert_eq!(result.after.roles.len(), 1);
        assert_eq!(result.after.roles[0].name, "VIP");
        assert!(result.synthetic_roles.contains_key(&ResourceKey("vip".to_string())));
        assert!(serde_json::to_string(&result).is_ok());
    }

    #[test]
    fn overwrite_threads_synthetic_role_id() {
        let vk = ResourceKey("verified".to_string());
        let ck = ResourceKey("gen".to_string());
        let normalized = NormalizedDesiredState {
            roles: vec![nrole("verified", "Verified")],
            channels: vec![nchannel("gen", "gen", vec![NormalizedOverwrite { target: NormalizedTarget::Role(vk.clone()), allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty() }])],
            ..Default::default()
        };
        let graph = OperationGraph { nodes: vec![
            node(0, Operation::CreateRole { key: vk.clone(), name: Some("Verified".to_string()), permissions: Some(Permissions::empty()) }),
            node(1, Operation::CreateChannel { key: ck.clone(), name: Some("gen".to_string()), channel_type: None, parent: None }),
            node(2, Operation::CreateOverwrite { channel: ck.clone(), target: NormalizedTarget::Role(vk.clone()), allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty() }),
        ] };
        let current = empty_guild();
        let resolver = InMemoryMatchResolver::new(&current);
        let result = apply(&current, &graph, &normalized, &resolver).unwrap();
        let synthetic_role = result.synthetic_roles[&vk];
        let ch = result.after.channels.iter().find(|c| c.name == "gen").unwrap();
        assert!(ch.overwrites.iter().any(|o| o.target == OverwriteTarget::Role(synthetic_role)));
    }

    #[test]
    fn missing_existing_key_errors() {
        let normalized = NormalizedDesiredState {
            channels: vec![nchannel("gen", "gen", vec![])],
            ..Default::default()
        };
        let graph = OperationGraph { nodes: vec![node(0, Operation::UpdateChannel { key: ResourceKey("gen".to_string()), name: Some("gen".to_string()), channel_type: None })] };
        let current = empty_guild();
        let resolver = InMemoryMatchResolver::new(&current);
        assert!(matches!(apply(&current, &graph, &normalized, &resolver), Err(VirtualApplyError::UnresolvedKey { .. })));
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p virtual-apply` → FAIL.

- [ ] **Step 4: apply.rs 구현**

`apply.rs` 테스트 위에:
```rust
use std::collections::BTreeMap;

use desired_compiler::{NormalizedDesiredState, NormalizedTarget};
use desired_state::ResourceKey;
use diff_engine::{ResolveResult, ResourceResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, GuildState, OverwriteTarget, PermissionOverwrite, Permissions,
    Role, RoleId, UserId,
};
use operation_graph::{OpId, Operation, OperationGraph};

use crate::error::VirtualApplyError;
use crate::result::VirtualApplyResult;

pub fn apply(
    current: &GuildState,
    graph: &OperationGraph,
    normalized: &NormalizedDesiredState,
    resolver: &impl ResourceResolver,
) -> Result<VirtualApplyResult, VirtualApplyError> {
    let mut ctx = ApplyContext::new(current, normalized, resolver);
    let order = graph.topological_order().map_err(|_| VirtualApplyError::GraphCycle)?;
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
    normalized: &'a NormalizedDesiredState,
    resolver: &'a R,
    next_id: u64,
    role_ids: BTreeMap<ResourceKey, RoleId>,
    channel_ids: BTreeMap<ResourceKey, ChannelId>,
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
            normalized,
            resolver,
            next_id: max_id + 1,
            role_ids: BTreeMap::new(),
            channel_ids: BTreeMap::new(),
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
            Operation::CreateRole { key, name, permissions } => {
                let id = RoleId(self.next_synthetic());
                self.role_ids.insert(key.clone(), id);
                self.synthetic_roles.insert(key.clone(), id);
                self.after.roles.push(Role {
                    id,
                    name: name.clone().unwrap_or_default(),
                    permissions: permissions.unwrap_or_default(),
                    position: 0,
                    managed: false,
                });
            }
            Operation::UpdateRole { key, name, permissions } => {
                let id = self.resolve_role(key)?;
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
                let id = self.resolve_role(key)?;
                self.after.roles.retain(|r| r.id != id);
            }
            Operation::CreateChannel { key, name, channel_type, parent } => {
                let id = ChannelId(self.next_synthetic());
                self.channel_ids.insert(key.clone(), id);
                self.synthetic_channels.insert(key.clone(), id);
                let parent_id = match parent {
                    Some(pk) => Some(self.resolve_channel(pk)?),
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
            Operation::UpdateChannel { key, name, channel_type } => {
                let id = self.resolve_channel(key)?;
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
                let id = self.resolve_channel(key)?;
                self.after.channels.retain(|c| c.id != id);
            }
            Operation::CreateOverwrite { channel, target, allow, deny }
            | Operation::UpdateOverwrite { channel, target, allow, deny } => {
                let channel_id = self.resolve_channel(channel)?;
                let ow_target = self.resolve_target(target)?;
                if let Some(ch) = self.after.channels.iter_mut().find(|c| c.id == channel_id) {
                    ch.overwrites.retain(|o| o.target != ow_target);
                    ch.overwrites.push(PermissionOverwrite { target: ow_target, allow: *allow, deny: *deny });
                } else {
                    self.warnings.push(format!("overwrite channel not found: {}", channel.0));
                }
            }
        }
        Ok(())
    }

    fn resolve_role(&mut self, key: &ResourceKey) -> Result<RoleId, VirtualApplyError> {
        if let Some(id) = self.role_ids.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nr = self
                .normalized
                .roles
                .iter()
                .find(|r| &r.identity.key == key)
                .ok_or_else(|| VirtualApplyError::MissingIdentity { key: key.0.clone() })?;
            self.resolver.resolve_role(&nr.identity, nr.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(role) => {
                self.role_ids.insert(key.clone(), role.id);
                Ok(role.id)
            }
            _ => Err(VirtualApplyError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    fn resolve_channel(&mut self, key: &ResourceKey) -> Result<ChannelId, VirtualApplyError> {
        if let Some(id) = self.channel_ids.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nc = self
                .normalized
                .channels
                .iter()
                .find(|c| &c.identity.key == key)
                .ok_or_else(|| VirtualApplyError::MissingIdentity { key: key.0.clone() })?;
            self.resolver.resolve_channel(&nc.identity, nc.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(ch) => {
                self.channel_ids.insert(key.clone(), ch.id);
                Ok(ch.id)
            }
            _ => Err(VirtualApplyError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    fn resolve_target(&mut self, target: &NormalizedTarget) -> Result<OverwriteTarget, VirtualApplyError> {
        match target {
            NormalizedTarget::Everyone => Ok(OverwriteTarget::Role(RoleId(self.after.guild.id.0))),
            NormalizedTarget::Role(key) => Ok(OverwriteTarget::Role(self.resolve_role(key)?)),
            NormalizedTarget::Member(id) => {
                let raw = id.parse::<u64>().map_err(|_| VirtualApplyError::UnresolvedKey { key: id.clone() })?;
                Ok(OverwriteTarget::Member(UserId(raw)))
            }
        }
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

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p virtual-apply && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(virtual-apply): add dry-run OperationGraph application"
```

- [ ] **Step 6: Task 보고**

---

### Task 2: 크라운 주얼 (전 파이프라인 end-to-end) + 최종 게이트

**Files:**
- Create: `crates/virtual-apply/tests/pipeline_scenario.rs`

**Interfaces:**
- Produces: DesiredState → compile → diff → graph → virtual-apply → simulator end-to-end.

- [ ] **Step 1: pipeline_scenario.rs — Cargo dev-dep에 simulator만 추가**

통합 테스트(`tests/`)는 crate의 일반 `[dependencies]`(desired-state/desired-compiler/operation-graph/diff-engine/discord-model)를 그대로 `use` 할 수 있음 → **재선언 불필요**. simulator만 일반 dep이 아니므로 `[dev-dependencies]`에 추가:
```toml
[dev-dependencies]
serde_json = { workspace = true }
simulator = { path = "../simulator" }
```
(Task 1의 `[dev-dependencies]`에 이미 serde_json이 있으면 simulator 줄만 추가.)

Create `crates/virtual-apply/tests/pipeline_scenario.rs`:
```rust
use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use operation_graph::compile_operations;
use simulator::{can_send, can_view};
use virtual_apply::apply;

fn before_guild() -> GuildState {
    GuildState {
        guild: Guild { id: GuildId(1), name: "srv".to_string(), owner_id: UserId(1) },
        roles: vec![Role { id: RoleId(1), name: "everyone".to_string(), permissions: Permissions::VIEW_CHANNEL, position: 0, managed: false }],
        channels: vec![Channel {
            id: ChannelId(500), name: "general".to_string(), channel_type: ChannelType::Text, parent_id: None, position: 0,
            overwrites: vec![PermissionOverwrite { target: OverwriteTarget::Role(RoleId(1)), allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty() }],
        }],
        members: vec![],
    }
}

fn desired() -> DesiredState {
    let verified = ResourceKey("verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(verified.clone(), AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] });
    DesiredState {
        roles: vec![RoleIntent { identity: Identity { key: verified, ..Default::default() }, name: Some("Verified".to_string()), permissions: Some(Permissions::empty()) }],
        channels: vec![ChannelIntent {
            identity: Identity { key: ResourceKey("general".to_string()), ..Default::default() },
            name: Some("general".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent { everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }), roles }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

#[test]
fn full_pipeline_to_after_state_and_simulation() {
    let before = before_guild();
    let normalized = compile(&desired()).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = compile_operations(&diff_result, &normalized).unwrap();

    let resolver = InMemoryMatchResolver::new(&before);
    let result = apply(&before, &graph, &normalized, &resolver).unwrap();
    let after = &result.after;

    let verified_id = result.synthetic_roles[&ResourceKey("verified".to_string())];
    let general = after.channels.iter().find(|c| c.name == "general").unwrap();

    assert!(after.roles.iter().any(|r| r.id == verified_id));
    assert!(general.overwrites.iter().any(|o| o.target == OverwriteTarget::Role(RoleId(1)) && o.deny.contains(Permissions::VIEW_CHANNEL)));
    assert!(general.overwrites.iter().any(|o| o.target == OverwriteTarget::Role(verified_id) && o.allow.contains(Permissions::VIEW_CHANNEL)));

    assert!(!can_view(after, &[], general));
    assert!(can_view(after, &[verified_id], general));
    assert!(can_send(after, &[verified_id], general));
}
```

- [ ] **Step 2: 통과 확인** — `cargo test -p virtual-apply --test pipeline_scenario`. re-export 누락 시 lib.rs 보완.

- [ ] **Step 3: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 실제 출력대로 보고.

- [ ] **Step 4: 커밋 + push + 보고**
```bash
git add -A
git commit -m "test(virtual-apply): add full pipeline scenario to after-state and simulation"
git push origin main
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] virtual-apply: apply(4입력), synthetic id, key 해소(synthetic/Resolver), 8 op 적용, VirtualApplyResult/Error
- [ ] synthetic threading: CreateRole synthetic id가 overwrite target에 연결
- [ ] **크라운 주얼**: before → compile→diff→graph→virtual-apply→simulator, general에 @everyone deny VIEW + verified allow VIEW+SEND, new can't view / verified can view+send
- [ ] 의존 방향·주석 없음·Task별 커밋·**main push**
