# Operation Graph Implementation Plan (Phase 5)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task가 크니 Task 안에서 여러 TDD 사이클/커밋 후 Task 끝에 보고.

**Goal:** `crates/operation-graph` — `compile_operations(&DiffResult, &NormalizedDesiredState) -> Result<OperationGraph, _>`. produces/consumes 심볼로 depends_on 자동 도출 + cycle detection + topological order. 선행으로 diff-engine 신규채널 overwrite 갭 수정.

**Architecture:** DiffResult의 각 변경 → OperationNode(kind+payload+produces/consumes). consume이 produce와 매칭 → depends_on 자동. 순수 Rust, 그래프 구조까지만.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), thiserror, path deps(diff-engine·desired-compiler·desired-state·discord-model).

## Global Constraints
> ⚠️ **주석 금지**: `//`, `///`, `//!` 없음. 코드 블록의 설명 문구 제거하고 구현.
- 의존: `operation-graph → {diff-engine, desired-compiler, desired-state, discord-model}`. 역방향 금지.
- 결정적 출력(노드 순서 = diff.changes 순서, topo는 id 오름차순 우선).
- 완료 게이트: build/test/clippy(-D warnings)/fmt.
- Task별 커밋, Task 끝에 보고.

---

### Task 1: 선행(diff-engine 신규채널 overwrite) + operation-graph 스캐폴드 + 타입

**Files:**
- Modify: `crates/diff-engine/src/diff.rs`
- Modify: `Cargo.toml`
- Create: `crates/operation-graph/Cargo.toml`, `src/lib.rs`, `src/symbol.rs`, `src/node.rs`, `src/error.rs`

**Interfaces:**
- Produces: 신규채널 diff가 overwrite Create 포함. `ResourceSymbol`, `OpId`, `Operation`, `OperationNode`, `OperationGraph`, `OperationGraphError`.

- [ ] **Step 1: diff-engine 갭 테스트**

`crates/diff-engine/src/diff.rs` 테스트 모듈에 추가:
```rust
    #[test]
    fn new_channel_emits_overwrite_creates() {
        let g = empty_guild();
        let desired = NormalizedDesiredState {
            channels: vec![nchannel("gen", "general", vec![NormalizedOverwrite {
                target: NormalizedTarget::Everyone,
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            }])],
            ..Default::default()
        };
        let d = diff(&desired, &InMemoryMatchResolver::new(&g));
        let overwrite_creates = d.changes.iter().filter(|c| matches!(c.target, DiffTarget::Overwrite { .. }) && c.op == ChangeOp::Create).count();
        assert_eq!(overwrite_creates, 1);
    }
```
(이 테스트에서 `nchannel`/`empty_guild`는 기존 Task 2/3 테스트 헬퍼 재사용.)

- [ ] **Step 2: 실패 확인** — `cargo test -p diff-engine` → FAIL(신규채널 overwrite 미생성).

- [ ] **Step 3: diff_channel Missing 분기 수정**

`diff.rs`의 `diff_channel` 함수 `ResolveResult::Missing`의 `else` 블록(Create channel push)을 아래로 교체:
```rust
                } else {
                    out.changes.push(DiffChange { op: ChangeOp::Create, target, changed: vec![] });
                    for ow in &channel.overwrites {
                        out.changes.push(DiffChange {
                            op: ChangeOp::Create,
                            target: DiffTarget::Overwrite { channel: channel.identity.key.clone(), target: ow.target.clone() },
                            changed: vec![],
                        });
                    }
                }
```

- [ ] **Step 4: 통과 + 커밋**
```bash
cargo test -p diff-engine && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "fix(diff-engine): emit overwrite creates for newly created channels"
```

- [ ] **Step 5: operation-graph 스캐폴드**

Root `Cargo.toml` members에 `"crates/operation-graph"` 추가.

Create `crates/operation-graph/Cargo.toml`:
```toml
[package]
name = "operation-graph"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }
desired-state = { path = "../desired-state" }
desired-compiler = { path = "../desired-compiler" }
diff-engine = { path = "../diff-engine" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/operation-graph/src/lib.rs`:
```rust
pub mod error;
pub mod node;
pub mod symbol;

pub use error::OperationGraphError;
pub use node::{OpId, Operation, OperationGraph, OperationNode};
pub use symbol::ResourceSymbol;
```

- [ ] **Step 6: 타입 테스트**

Create `crates/operation-graph/src/symbol.rs`:
```rust
use desired_state::ResourceKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceSymbol {
    Role(ResourceKey),
    Channel(ResourceKey),
}
```

Create `crates/operation-graph/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum OperationGraphError {
    #[error("diff has {0} unresolved conflict(s)")]
    DiffHasConflicts(usize),
    #[error("missing payload for {key}")]
    MissingPayload { key: String },
    #[error("unsupported diff change")]
    UnsupportedChange,
    #[error("dependency cycle detected")]
    DependencyCycle,
}
```

Create `crates/operation-graph/src/node.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::ResourceKey;
    use discord_model::Permissions;

    #[test]
    fn node_roundtrip() {
        let node = OperationNode {
            id: OpId(0),
            operation: Operation::CreateRole {
                key: ResourceKey("r".to_string()),
                name: Some("R".to_string()),
                permissions: Some(Permissions::empty()),
            },
            produces: vec![ResourceSymbol::Role(ResourceKey("r".to_string()))],
            consumes: vec![],
            depends_on: vec![],
        };
        let json = serde_json::to_string(&node).unwrap();
        assert_eq!(serde_json::from_str::<OperationNode>(&json).unwrap(), node);
    }
}
```

`node.rs` 테스트 위에:
```rust
use desired_compiler::NormalizedTarget;
use desired_state::ResourceKey;
use discord_model::{ChannelType, Permissions};
use serde::{Deserialize, Serialize};

use crate::symbol::ResourceSymbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OpId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    CreateRole { key: ResourceKey, name: Option<String>, permissions: Option<Permissions> },
    UpdateRole { key: ResourceKey, name: Option<String>, permissions: Option<Permissions> },
    DeleteRole { key: ResourceKey },
    CreateChannel { key: ResourceKey, name: Option<String>, channel_type: Option<ChannelType>, parent: Option<ResourceKey> },
    UpdateChannel { key: ResourceKey, name: Option<String>, channel_type: Option<ChannelType> },
    DeleteChannel { key: ResourceKey },
    CreateOverwrite { channel: ResourceKey, target: NormalizedTarget, allow: Permissions, deny: Permissions },
    UpdateOverwrite { channel: ResourceKey, target: NormalizedTarget, allow: Permissions, deny: Permissions },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationNode {
    pub id: OpId,
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<ResourceSymbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumes: Vec<ResourceSymbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<OpId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationGraph {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<OperationNode>,
}
```

- [ ] **Step 7: 통과 + 커밋**
```bash
cargo test -p operation-graph && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(operation-graph): scaffold crate with node and symbol types"
```

- [ ] **Step 8: Task 보고** (커밋 2개: diff-engine fix + operation-graph scaffold)

---

### Task 2: compile_operations (노드 생성)

**Files:**
- Create: `crates/operation-graph/src/compile.rs`
- Modify: `crates/operation-graph/src/lib.rs`

**Interfaces:**
- Produces: `compile_operations(&DiffResult, &NormalizedDesiredState) -> Result<OperationGraph, OperationGraphError>` — 노드 생성(payload/produces/consumes). depends_on은 아직 빈 채(Task 3).

- [ ] **Step 1: 노드 생성 테스트**

Create `crates/operation-graph/src/compile.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use desired_compiler::{NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole, NormalizedTarget};
    use desired_state::{Identity, ResourceKey};
    use diff_engine::{ChangeOp, DiffChange, DiffConflict, DiffResult, DiffTarget};
    use discord_model::Permissions;

    fn nrole(key: &str) -> NormalizedRole {
        NormalizedRole { identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() }, name: Some(key.to_string()), permissions: Some(Permissions::empty()) }
    }
    fn nchannel(key: &str, overwrites: Vec<NormalizedOverwrite>) -> NormalizedChannel {
        NormalizedChannel { identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() }, name: Some(key.to_string()), channel_type: None, parent: None, overwrites }
    }
    fn create(target: DiffTarget) -> DiffChange {
        DiffChange { op: ChangeOp::Create, target, changed: vec![] }
    }

    #[test]
    fn create_role_produces_symbol() {
        let desired = NormalizedDesiredState { roles: vec![nrole("r")], ..Default::default() };
        let diff = DiffResult { changes: vec![create(DiffTarget::Role { key: ResourceKey("r".to_string()) })], ..Default::default() };
        let g = compile_operations(&diff, &desired).unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].produces, vec![ResourceSymbol::Role(ResourceKey("r".to_string()))]);
    }

    #[test]
    fn create_overwrite_consumes_channel_and_role() {
        let ow = NormalizedOverwrite { target: NormalizedTarget::Role(ResourceKey("r".to_string())), allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty() };
        let desired = NormalizedDesiredState { channels: vec![nchannel("c", vec![ow])], ..Default::default() };
        let diff = DiffResult { changes: vec![create(DiffTarget::Overwrite { channel: ResourceKey("c".to_string()), target: NormalizedTarget::Role(ResourceKey("r".to_string())) })], ..Default::default() };
        let g = compile_operations(&diff, &desired).unwrap();
        assert!(g.nodes[0].consumes.contains(&ResourceSymbol::Channel(ResourceKey("c".to_string()))));
        assert!(g.nodes[0].consumes.contains(&ResourceSymbol::Role(ResourceKey("r".to_string()))));
    }

    #[test]
    fn conflicts_block_compile() {
        let diff = DiffResult { conflicts: vec![DiffConflict { target: DiffTarget::Role { key: ResourceKey("r".to_string()) }, reason: "x".to_string() }], ..Default::default() };
        let desired = NormalizedDesiredState::default();
        assert!(matches!(compile_operations(&diff, &desired), Err(OperationGraphError::DiffHasConflicts(1))));
    }

    #[test]
    fn noop_produces_no_node() {
        let diff = DiffResult { changes: vec![DiffChange { op: ChangeOp::NoOp, target: DiffTarget::Role { key: ResourceKey("r".to_string()) }, changed: vec![] }], ..Default::default() };
        let desired = NormalizedDesiredState { roles: vec![nrole("r")], ..Default::default() };
        assert!(compile_operations(&diff, &desired).unwrap().nodes.is_empty());
    }
}
```

Modify `lib.rs`: `pub mod compile;` + `pub use compile::compile_operations;`.

- [ ] **Step 2: 실패 확인** — `cargo test -p operation-graph` → FAIL.

- [ ] **Step 3: compile.rs 구현**

`compile.rs` 테스트 위에:
```rust
use desired_compiler::{NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole, NormalizedTarget};
use desired_state::ResourceKey;
use diff_engine::{ChangeOp, DiffChange, DiffResult, DiffTarget};

use crate::error::OperationGraphError;
use crate::node::{OpId, Operation, OperationGraph, OperationNode};
use crate::symbol::ResourceSymbol;

pub fn compile_operations(diff: &DiffResult, desired: &NormalizedDesiredState) -> Result<OperationGraph, OperationGraphError> {
    if !diff.conflicts.is_empty() {
        return Err(OperationGraphError::DiffHasConflicts(diff.conflicts.len()));
    }
    let mut nodes = Vec::new();
    let mut next_id = 0u32;
    for change in &diff.changes {
        if change.op == ChangeOp::NoOp {
            continue;
        }
        let (operation, produces, consumes) = build_operation(change, desired)?;
        nodes.push(OperationNode { id: OpId(next_id), operation, produces, consumes, depends_on: Vec::new() });
        next_id += 1;
    }
    Ok(OperationGraph { nodes })
}

fn build_operation(change: &DiffChange, desired: &NormalizedDesiredState) -> Result<(Operation, Vec<ResourceSymbol>, Vec<ResourceSymbol>), OperationGraphError> {
    match (change.op, &change.target) {
        (ChangeOp::Create, DiffTarget::Role { key }) => {
            let r = find_role(desired, key)?;
            Ok((Operation::CreateRole { key: key.clone(), name: r.name.clone(), permissions: r.permissions }, vec![ResourceSymbol::Role(key.clone())], vec![]))
        }
        (ChangeOp::Update, DiffTarget::Role { key }) => {
            let r = find_role(desired, key)?;
            Ok((Operation::UpdateRole { key: key.clone(), name: r.name.clone(), permissions: r.permissions }, vec![], vec![]))
        }
        (ChangeOp::Delete, DiffTarget::Role { key }) => {
            Ok((Operation::DeleteRole { key: key.clone() }, vec![], vec![]))
        }
        (ChangeOp::Create, DiffTarget::Channel { key }) => {
            let c = find_channel(desired, key)?;
            let consumes = c.parent.as_ref().map(|p| vec![ResourceSymbol::Channel(p.clone())]).unwrap_or_default();
            Ok((Operation::CreateChannel { key: key.clone(), name: c.name.clone(), channel_type: c.channel_type, parent: c.parent.clone() }, vec![ResourceSymbol::Channel(key.clone())], consumes))
        }
        (ChangeOp::Update, DiffTarget::Channel { key }) => {
            let c = find_channel(desired, key)?;
            Ok((Operation::UpdateChannel { key: key.clone(), name: c.name.clone(), channel_type: c.channel_type }, vec![], vec![]))
        }
        (ChangeOp::Delete, DiffTarget::Channel { key }) => {
            Ok((Operation::DeleteChannel { key: key.clone() }, vec![], vec![]))
        }
        (ChangeOp::Create, DiffTarget::Overwrite { channel, target }) => {
            let ow = find_overwrite(desired, channel, target)?;
            Ok((Operation::CreateOverwrite { channel: channel.clone(), target: target.clone(), allow: ow.allow, deny: ow.deny }, vec![], overwrite_consumes(channel, target)))
        }
        (ChangeOp::Update, DiffTarget::Overwrite { channel, target }) => {
            let ow = find_overwrite(desired, channel, target)?;
            Ok((Operation::UpdateOverwrite { channel: channel.clone(), target: target.clone(), allow: ow.allow, deny: ow.deny }, vec![], overwrite_consumes(channel, target)))
        }
        _ => Err(OperationGraphError::UnsupportedChange),
    }
}

fn overwrite_consumes(channel: &ResourceKey, target: &NormalizedTarget) -> Vec<ResourceSymbol> {
    let mut consumes = vec![ResourceSymbol::Channel(channel.clone())];
    if let NormalizedTarget::Role(rk) = target {
        consumes.push(ResourceSymbol::Role(rk.clone()));
    }
    consumes
}

fn find_role<'a>(desired: &'a NormalizedDesiredState, key: &ResourceKey) -> Result<&'a NormalizedRole, OperationGraphError> {
    desired.roles.iter().find(|r| &r.identity.key == key).ok_or_else(|| OperationGraphError::MissingPayload { key: key.0.clone() })
}

fn find_channel<'a>(desired: &'a NormalizedDesiredState, key: &ResourceKey) -> Result<&'a NormalizedChannel, OperationGraphError> {
    desired.channels.iter().find(|c| &c.identity.key == key).ok_or_else(|| OperationGraphError::MissingPayload { key: key.0.clone() })
}

fn find_overwrite<'a>(desired: &'a NormalizedDesiredState, channel: &ResourceKey, target: &NormalizedTarget) -> Result<&'a NormalizedOverwrite, OperationGraphError> {
    let ch = find_channel(desired, channel)?;
    ch.overwrites.iter().find(|o| &o.target == target).ok_or_else(|| OperationGraphError::MissingPayload { key: channel.0.clone() })
}
```

- [ ] **Step 4: 통과 + 커밋**
```bash
cargo test -p operation-graph && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(operation-graph): compile diff into operation nodes"
```

- [ ] **Step 5: Task 보고**

---

### Task 3: depends_on 자동 도출 + topological/rollback order + cycle

**Files:**
- Create: `crates/operation-graph/src/order.rs`
- Modify: `crates/operation-graph/src/compile.rs`, `crates/operation-graph/src/lib.rs`

**Interfaces:**
- Produces: `compile_operations`가 depends_on 채우고 cycle 시 에러. `OperationGraph::topological_order()`, `rollback_order()`.

- [ ] **Step 1: 테스트 추가**

`compile.rs` 테스트 모듈에 추가:
```rust
    #[test]
    fn overwrite_depends_on_role_create() {
        let ow = NormalizedOverwrite { target: NormalizedTarget::Role(ResourceKey("r".to_string())), allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty() };
        let desired = NormalizedDesiredState { roles: vec![nrole("r")], channels: vec![nchannel("c", vec![ow])], ..Default::default() };
        let diff = DiffResult { changes: vec![
            create(DiffTarget::Role { key: ResourceKey("r".to_string()) }),
            create(DiffTarget::Channel { key: ResourceKey("c".to_string()) }),
            create(DiffTarget::Overwrite { channel: ResourceKey("c".to_string()), target: NormalizedTarget::Role(ResourceKey("r".to_string())) }),
        ], ..Default::default() };
        let g = compile_operations(&diff, &desired).unwrap();
        let role_id = g.nodes.iter().find(|n| matches!(&n.operation, Operation::CreateRole { .. })).unwrap().id;
        let ow_node = g.nodes.iter().find(|n| matches!(&n.operation, Operation::CreateOverwrite { .. })).unwrap();
        assert!(ow_node.depends_on.contains(&role_id));
        assert!(g.topological_order().is_ok());
    }
```

Create `crates/operation-graph/src/order.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::node::{OpId, Operation, OperationGraph, OperationNode};
    use desired_state::ResourceKey;

    fn node(id: u32, deps: Vec<u32>) -> OperationNode {
        OperationNode {
            id: OpId(id),
            operation: Operation::DeleteRole { key: ResourceKey(format!("k{id}")) },
            produces: vec![],
            consumes: vec![],
            depends_on: deps.into_iter().map(OpId).collect(),
        }
    }

    #[test]
    fn topo_and_rollback() {
        let g = OperationGraph { nodes: vec![node(0, vec![]), node(1, vec![0]), node(2, vec![1])] };
        assert_eq!(g.topological_order().unwrap(), vec![OpId(0), OpId(1), OpId(2)]);
        assert_eq!(g.rollback_order().unwrap(), vec![OpId(2), OpId(1), OpId(0)]);
    }

    #[test]
    fn cycle_detected() {
        let g = OperationGraph { nodes: vec![node(0, vec![1]), node(1, vec![0])] };
        assert!(g.topological_order().is_err());
    }
}
```

Modify `lib.rs`: `pub mod order;` (order.rs는 impl 블록만 추가, re-export 불필요).

- [ ] **Step 2: 실패 확인** — `cargo test -p operation-graph` → FAIL.

- [ ] **Step 3: order.rs 구현**

`order.rs` 테스트 위에:
```rust
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::error::OperationGraphError;
use crate::node::{OpId, OperationGraph};

impl OperationGraph {
    pub fn topological_order(&self) -> Result<Vec<OpId>, OperationGraphError> {
        let mut indegree: HashMap<OpId, usize> = self.nodes.iter().map(|n| (n.id, 0usize)).collect();
        let mut dependents: HashMap<OpId, Vec<OpId>> = HashMap::new();
        for n in &self.nodes {
            for dep in &n.depends_on {
                if let Some(d) = indegree.get_mut(&n.id) {
                    *d += 1;
                }
                dependents.entry(*dep).or_default().push(n.id);
            }
        }
        let mut heap: BinaryHeap<Reverse<OpId>> = self
            .nodes
            .iter()
            .filter(|n| indegree.get(&n.id).copied().unwrap_or(0) == 0)
            .map(|n| Reverse(n.id))
            .collect();
        let mut order = Vec::new();
        while let Some(Reverse(id)) = heap.pop() {
            order.push(id);
            if let Some(deps) = dependents.get(&id) {
                for d in deps {
                    if let Some(e) = indegree.get_mut(d) {
                        *e -= 1;
                        if *e == 0 {
                            heap.push(Reverse(*d));
                        }
                    }
                }
            }
        }
        if order.len() == self.nodes.len() {
            Ok(order)
        } else {
            Err(OperationGraphError::DependencyCycle)
        }
    }

    pub fn rollback_order(&self) -> Result<Vec<OpId>, OperationGraphError> {
        let mut order = self.topological_order()?;
        order.reverse();
        Ok(order)
    }
}
```

- [ ] **Step 4: compile.rs에 depends_on 도출 추가**

`compile.rs`의 `compile_operations` 마지막 `Ok(OperationGraph { nodes })`를 아래로 교체:
```rust
    derive_dependencies(&mut nodes);
    let graph = OperationGraph { nodes };
    graph.topological_order()?;
    Ok(graph)
```

그리고 `compile.rs`에 함수 + import 추가:
```rust
use std::collections::HashMap;
```
```rust
fn derive_dependencies(nodes: &mut [OperationNode]) {
    let mut producers: HashMap<ResourceSymbol, OpId> = HashMap::new();
    for node in nodes.iter() {
        for sym in &node.produces {
            producers.insert(sym.clone(), node.id);
        }
    }
    for node in nodes.iter_mut() {
        let mut deps: Vec<OpId> = node
            .consumes
            .iter()
            .filter_map(|sym| producers.get(sym).copied())
            .filter(|dep| *dep != node.id)
            .collect();
        deps.sort();
        deps.dedup();
        node.depends_on = deps;
    }
}
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p operation-graph && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(operation-graph): derive dependencies and add topological ordering"
```

- [ ] **Step 6: Task 보고**

---

### Task 4: 통합 크라운 주얼 + 최종 게이트

**Files:**
- Create: `crates/operation-graph/tests/verification_scenario.rs`

**Interfaces:**
- Produces: DesiredState → compile → diff(empty) → operation graph. verified overwrite가 role/channel create에 자동 depends_on.

- [ ] **Step 1: 통합 테스트**

Create `crates/operation-graph/tests/verification_scenario.rs`:
```rust
use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{ChannelType, Guild, GuildId, GuildState, Permissions, UserId};
use operation_graph::{compile_operations, Operation};

fn desired() -> DesiredState {
    let verified = ResourceKey("verified_member".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(verified.clone(), AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] });
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
            access: Some(AccessIntent { everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }), roles }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

fn empty_guild() -> GuildState {
    GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles: vec![], channels: vec![], members: vec![] }
}

#[test]
fn overwrite_auto_depends_on_creates() {
    let normalized = compile(&desired()).unwrap();
    let d = diff(&normalized, &InMemoryMatchResolver::new(&empty_guild()));
    let graph = compile_operations(&d, &normalized).unwrap();

    let role_id = graph.nodes.iter().find(|n| matches!(&n.operation, Operation::CreateRole { .. })).unwrap().id;
    let channel_id = graph.nodes.iter().find(|n| matches!(&n.operation, Operation::CreateChannel { .. })).unwrap().id;

    let verified_ow = graph.nodes.iter().find(|n| matches!(&n.operation,
        Operation::CreateOverwrite { target: desired_compiler::NormalizedTarget::Role(k), .. } if k.0 == "verified_member")).unwrap();

    assert!(verified_ow.depends_on.contains(&role_id));
    assert!(verified_ow.depends_on.contains(&channel_id));
    assert!(graph.topological_order().is_ok());
}
```

- [ ] **Step 2: 통과 확인** — `cargo test -p operation-graph --test verification_scenario`. re-export 누락 시 lib.rs 보완(특히 `Operation`).

- [ ] **Step 3: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트는 실제 출력대로 보고.

- [ ] **Step 4: 커밋 + 보고**
```bash
git add -A
git commit -m "test(operation-graph): add verification scenario dependency fixture"
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 전부 통과
- [ ] diff-engine: 신규 채널 overwrite create 갭 수정
- [ ] operation-graph: ResourceSymbol/Operation/OperationNode/OperationGraph/OperationGraphError, `compile_operations`
- [ ] produces/consumes → depends_on 자동 도출, cycle detection, topological_order/rollback_order
- [ ] conflict diff → DiffHasConflicts 에러
- [ ] **크라운 주얼**: verified overwrite 노드가 role/channel create에 자동 depends_on, topo 유효
- [ ] 의존 `operation-graph → {diff-engine, desired-compiler, desired-state, discord-model}`, 주석 없음
- [ ] Task별 커밋
