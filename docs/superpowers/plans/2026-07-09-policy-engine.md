# Policy Engine Implementation Plan (Phase 7)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고.

**Goal:** `crates/policy-engine` — `PolicyEngine::evaluate(&OperationGraph) -> PolicyDecision`. pluggable PolicyRule + 3규칙(privileged/destructive/everyone) + verdict 집계. 선행으로 discord-model 권한 비트 2개 추가.

**Architecture:** PolicyRule 트레이트 → 각 rule이 OperationGraph에서 Finding 반환 → 엔진이 max verdict 집계. 순수 Rust.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), operation-graph·desired-compiler·desired-state·discord-model.

## Global Constraints
> ⚠️ **주석 금지**. 비트 연산은 `.bits()` 명시(truncation 회피).
- 의존: `policy-engine → {operation-graph, desired-compiler, desired-state, discord-model}`. 역방향 금지.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋, Task 끝에 보고.

---

### Task 1: 선행(discord-model 비트) + 스캐폴드 + 엔진 코어

**Files:**
- Modify: `crates/discord-model/src/permissions.rs`, `Cargo.toml`
- Create: `crates/policy-engine/Cargo.toml`, `src/{lib.rs, verdict.rs, finding.rs, engine.rs, rule.rs}`

**Interfaces:**
- Produces: Permissions +2비트. `Verdict`, `Finding`, `PolicyDecision`, `PolicyRule` trait, `PolicyEngine`(new/evaluate, 집계).

- [ ] **Step 1: discord-model 비트 테스트 + 추가**

`crates/discord-model/src/permissions.rs` 테스트 모듈에 추가:
```rust
    #[test]
    fn moderation_permission_bits() {
        assert_eq!(Permissions::MENTION_EVERYONE.bits(), 1 << 17);
        assert_eq!(Permissions::MODERATE_MEMBERS.bits(), 1 << 40);
    }
```
그리고 `bitflags!` 블록에 두 줄 추가(비트 순서 유지): `READ_MESSAGE_HISTORY = 1 << 16` 뒤에 `const MENTION_EVERYONE = 1 << 17;`, `MANAGE_ROLES = 1 << 28` 뒤에 `const MODERATE_MEMBERS = 1 << 40;`.

Run: `cargo test -p discord-model` → 통과. 커밋:
```bash
cargo fmt --all && git add -A && git commit -m "feat(discord-model): add MENTION_EVERYONE and MODERATE_MEMBERS bits"
```

- [ ] **Step 2: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/policy-engine"` 추가.

Create `crates/policy-engine/Cargo.toml`:
```toml
[package]
name = "policy-engine"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
operation-graph = { path = "../operation-graph" }
desired-compiler = { path = "../desired-compiler" }
desired-state = { path = "../desired-state" }
discord-model = { path = "../discord-model" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/policy-engine/src/lib.rs` (Task 1은 rules 없이. rules 모듈은 Task 2에서 추가):
```rust
pub mod engine;
pub mod finding;
pub mod rule;
pub mod verdict;

pub use engine::{PolicyDecision, PolicyEngine};
pub use finding::Finding;
pub use rule::PolicyRule;
pub use verdict::Verdict;
```

- [ ] **Step 3: verdict/finding/rule 구현**

Create `crates/policy-engine/src/verdict.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Allow,
    Warn,
    RequireApproval,
    RequireSecondApproval,
    Deny,
}
```

Create `crates/policy-engine/src/finding.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::verdict::Verdict;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub verdict: Verdict,
    pub target: String,
    pub message: String,
}
```

Create `crates/policy-engine/src/rule.rs`:
```rust
use operation_graph::OperationGraph;

use crate::finding::Finding;

pub trait PolicyRule {
    fn id(&self) -> &str;
    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding>;
}
```

- [ ] **Step 4: engine 테스트 + 구현**

Create `crates/policy-engine/src/engine.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Finding;
    use crate::rule::PolicyRule;
    use crate::verdict::Verdict;
    use operation_graph::OperationGraph;

    struct MockRule(Verdict);
    impl PolicyRule for MockRule {
        fn id(&self) -> &str {
            "mock"
        }
        fn evaluate(&self, _g: &OperationGraph) -> Vec<Finding> {
            vec![Finding { rule_id: "mock".to_string(), verdict: self.0, target: "t".to_string(), message: "m".to_string() }]
        }
    }

    #[test]
    fn verdict_ordering() {
        assert!(Verdict::Deny > Verdict::RequireApproval);
        assert!(Verdict::RequireApproval > Verdict::Warn);
        assert!(Verdict::Warn > Verdict::Allow);
    }

    #[test]
    fn empty_engine_allows() {
        let engine = PolicyEngine::new(vec![]);
        assert_eq!(engine.evaluate(&OperationGraph::default()).verdict, Verdict::Allow);
    }

    #[test]
    fn aggregates_max_verdict() {
        let engine = PolicyEngine::new(vec![Box::new(MockRule(Verdict::Warn)), Box::new(MockRule(Verdict::Deny))]);
        let decision = engine.evaluate(&OperationGraph::default());
        assert_eq!(decision.verdict, Verdict::Deny);
        assert_eq!(decision.findings.len(), 2);
    }
}
```

`engine.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use operation_graph::OperationGraph;

use crate::finding::Finding;
use crate::rule::PolicyRule;
use crate::verdict::Verdict;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub verdict: Verdict,
    pub findings: Vec<Finding>,
}

pub struct PolicyEngine {
    rules: Vec<Box<dyn PolicyRule + Send + Sync>>,
}

impl PolicyEngine {
    pub fn new(rules: Vec<Box<dyn PolicyRule + Send + Sync>>) -> Self {
        Self { rules }
    }

    pub fn evaluate(&self, graph: &OperationGraph) -> PolicyDecision {
        let mut findings = Vec::new();
        for rule in &self.rules {
            findings.extend(rule.evaluate(graph));
        }
        let verdict = findings.iter().map(|f| f.verdict).max().unwrap_or(Verdict::Allow);
        PolicyDecision { verdict, findings }
    }
}
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p policy-engine && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(policy-engine): scaffold with Verdict, Finding, PolicyRule, PolicyEngine"
```

- [ ] **Step 6: Task 보고** (커밋 2개: discord-model 비트 + policy-engine 스캐폴드)

---

### Task 2: 3규칙 + with_default_rules

**Files:**
- Create: `crates/policy-engine/src/rules.rs`
- Modify: `crates/policy-engine/src/lib.rs`, `crates/policy-engine/src/engine.rs`

**Interfaces:**
- Produces: `PrivilegedPermissionRule`/`DestructiveOperationRule`/`EveryoneChangeRule`, `PolicyEngine::with_default_rules`.

- [ ] **Step 1: 규칙 테스트**

Create `crates/policy-engine/src/rules.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::PolicyRule;
    use crate::verdict::Verdict;
    use desired_compiler::NormalizedTarget;
    use desired_state::ResourceKey;
    use discord_model::Permissions;
    use operation_graph::{OpId, Operation, OperationGraph, OperationNode};

    fn node(op: Operation) -> OperationNode {
        OperationNode { id: OpId(0), operation: op, produces: vec![], consumes: vec![], depends_on: vec![] }
    }
    fn graph(ops: Vec<Operation>) -> OperationGraph {
        OperationGraph { nodes: ops.into_iter().map(node).collect() }
    }

    #[test]
    fn privileged_admin_denied() {
        let g = graph(vec![Operation::CreateRole { key: ResourceKey("a".to_string()), name: Some("Admin".to_string()), permissions: Some(Permissions::ADMINISTRATOR) }]);
        let f = PrivilegedPermissionRule.evaluate(&g);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].verdict, Verdict::Deny);
    }

    #[test]
    fn privileged_safe_role_ok() {
        let g = graph(vec![Operation::CreateRole { key: ResourceKey("vip".to_string()), name: Some("VIP".to_string()), permissions: Some(Permissions::empty()) }]);
        assert!(PrivilegedPermissionRule.evaluate(&g).is_empty());
    }

    #[test]
    fn privileged_in_overwrite_denied() {
        let g = graph(vec![Operation::CreateOverwrite { channel: ResourceKey("c".to_string()), target: NormalizedTarget::Role(ResourceKey("r".to_string())), allow: Permissions::MANAGE_ROLES, deny: Permissions::empty() }]);
        assert_eq!(PrivilegedPermissionRule.evaluate(&g)[0].verdict, Verdict::Deny);
    }

    #[test]
    fn destructive_verdicts() {
        let g = graph(vec![
            Operation::DeleteRole { key: ResourceKey("r".to_string()) },
            Operation::DeleteChannel { key: ResourceKey("c".to_string()) },
        ]);
        let f = DestructiveOperationRule.evaluate(&g);
        assert!(f.iter().any(|x| x.verdict == Verdict::RequireApproval));
        assert!(f.iter().any(|x| x.verdict == Verdict::RequireSecondApproval));
    }

    #[test]
    fn everyone_change_requires_approval() {
        let g = graph(vec![Operation::CreateOverwrite { channel: ResourceKey("gen".to_string()), target: NormalizedTarget::Everyone, allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL }]);
        let f = EveryoneChangeRule.evaluate(&g);
        assert_eq!(f[0].verdict, Verdict::RequireApproval);
        let g2 = graph(vec![Operation::CreateOverwrite { channel: ResourceKey("gen".to_string()), target: NormalizedTarget::Role(ResourceKey("r".to_string())), allow: Permissions::empty(), deny: Permissions::empty() }]);
        assert!(EveryoneChangeRule.evaluate(&g2).is_empty());
    }
}
```

Modify `lib.rs`: `pub mod rules;` + `pub use rules::{DestructiveOperationRule, EveryoneChangeRule, PrivilegedPermissionRule};`.

- [ ] **Step 2: 실패 확인** — `cargo test -p policy-engine` → FAIL.

- [ ] **Step 3: 규칙 구현**

`rules.rs` 테스트 위에:
```rust
use desired_compiler::NormalizedTarget;
use discord_model::Permissions;
use operation_graph::{Operation, OperationGraph};

use crate::finding::Finding;
use crate::rule::PolicyRule;
use crate::verdict::Verdict;

fn privileged_mask() -> Permissions {
    Permissions::ADMINISTRATOR
        | Permissions::MANAGE_GUILD
        | Permissions::MANAGE_ROLES
        | Permissions::MANAGE_CHANNELS
        | Permissions::KICK_MEMBERS
        | Permissions::BAN_MEMBERS
        | Permissions::MENTION_EVERYONE
        | Permissions::MODERATE_MEMBERS
}

pub struct PrivilegedPermissionRule;

impl PolicyRule for PrivilegedPermissionRule {
    fn id(&self) -> &str {
        "privileged-permission"
    }

    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding> {
        let mask = privileged_mask();
        let mut findings = Vec::new();
        for node in &graph.nodes {
            let (granted, target) = match &node.operation {
                Operation::CreateRole { key, permissions, .. }
                | Operation::UpdateRole { key, permissions, .. } => (*permissions, format!("role:{}", key.0)),
                Operation::CreateOverwrite { channel, allow, .. }
                | Operation::UpdateOverwrite { channel, allow, .. } => {
                    (Some(*allow), format!("overwrite:{}", channel.0))
                }
                _ => continue,
            };
            if let Some(perms) = granted {
                if perms.bits() & mask.bits() != 0 {
                    findings.push(Finding {
                        rule_id: self.id().to_string(),
                        verdict: Verdict::Deny,
                        target,
                        message: "privileged permission granted".to_string(),
                    });
                }
            }
        }
        findings
    }
}

pub struct DestructiveOperationRule;

impl PolicyRule for DestructiveOperationRule {
    fn id(&self) -> &str {
        "destructive-operation"
    }

    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding> {
        let mut findings = Vec::new();
        for node in &graph.nodes {
            let (verdict, target, message) = match &node.operation {
                Operation::DeleteRole { key } => (Verdict::RequireApproval, format!("role:{}", key.0), "role deletion"),
                Operation::DeleteChannel { key } => (Verdict::RequireSecondApproval, format!("channel:{}", key.0), "channel deletion"),
                _ => continue,
            };
            findings.push(Finding { rule_id: self.id().to_string(), verdict, target, message: message.to_string() });
        }
        findings
    }
}

pub struct EveryoneChangeRule;

impl PolicyRule for EveryoneChangeRule {
    fn id(&self) -> &str {
        "everyone-change"
    }

    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding> {
        let mut findings = Vec::new();
        for node in &graph.nodes {
            let (channel, target) = match &node.operation {
                Operation::CreateOverwrite { channel, target, .. }
                | Operation::UpdateOverwrite { channel, target, .. } => (channel, target),
                _ => continue,
            };
            if matches!(target, NormalizedTarget::Everyone) {
                findings.push(Finding {
                    rule_id: self.id().to_string(),
                    verdict: Verdict::RequireApproval,
                    target: format!("overwrite:{}:everyone", channel.0),
                    message: "changes @everyone access".to_string(),
                });
            }
        }
        findings
    }
}
```

- [ ] **Step 4: with_default_rules 추가**

`engine.rs`의 `impl PolicyEngine`에 추가:
```rust
    pub fn with_default_rules() -> Self {
        Self::new(vec![
            Box::new(crate::rules::PrivilegedPermissionRule),
            Box::new(crate::rules::DestructiveOperationRule),
            Box::new(crate::rules::EveryoneChangeRule),
        ])
    }
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p policy-engine && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(policy-engine): add privileged, destructive, and everyone rules"
```

- [ ] **Step 6: Task 보고**

---

### Task 3: 통합 (no-admin 차단 + 인증 시나리오) + 최종 게이트

**Files:**
- Create: `crates/policy-engine/tests/scenario.rs`

**Interfaces:**
- Produces: DesiredState → compile → diff → graph → PolicyEngine. no-admin→Deny, verify-gate→RequireApproval.

- [ ] **Step 1: 통합 테스트**

Create `crates/policy-engine/tests/scenario.rs`:
```rust
use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{ChannelType, Guild, GuildId, GuildState, Permissions, UserId};
use operation_graph::compile_operations;
use policy_engine::{PolicyEngine, Verdict};

fn empty_guild() -> GuildState {
    GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles: vec![], channels: vec![], members: vec![] }
}

fn decide(desired: &DesiredState) -> Verdict {
    let normalized = compile(desired).unwrap();
    let guild = empty_guild();
    let d = diff(&normalized, &InMemoryMatchResolver::new(&guild));
    let graph = compile_operations(&d, &normalized).unwrap();
    PolicyEngine::with_default_rules().evaluate(&graph).verdict
}

#[test]
fn admin_grant_is_denied() {
    let desired = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: ResourceKey("admin".to_string()), ..Default::default() },
            name: Some("Administrator".to_string()),
            permissions: Some(Permissions::ADMINISTRATOR),
        }],
        ..Default::default()
    };
    assert_eq!(decide(&desired), Verdict::Deny);
}

#[test]
fn verification_scenario_requires_approval() {
    let verified = ResourceKey("verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(verified.clone(), AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] });
    let desired = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: verified, ..Default::default() },
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
    };
    assert_eq!(decide(&desired), Verdict::RequireApproval);
}
```
> `scenario.rs`는 diff-engine을 쓰므로 `crates/policy-engine/Cargo.toml` [dev-dependencies]에 `diff-engine = { path = "../diff-engine" }` 추가.

- [ ] **Step 2: 통과 확인** — `cargo test -p policy-engine --test scenario`. re-export 누락 시 lib.rs 보완.

- [ ] **Step 3: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 실제 출력대로 보고.

- [ ] **Step 4: 커밋 + 보고**
```bash
git add -A
git commit -m "test(policy-engine): add admin-deny and verification scenario integration"
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] discord-model: MENTION_EVERYONE/MODERATE_MEMBERS 비트
- [ ] policy-engine: Verdict/Finding/PolicyDecision/PolicyRule/PolicyEngine
- [ ] 3규칙 + with_default_rules, verdict max 집계
- [ ] **no-admin 차단**: ADMINISTRATOR 부여 → Verdict::Deny
- [ ] 인증 시나리오 → Verdict::RequireApproval(@everyone 변경)
- [ ] 의존 방향·주석 없음·Task별 커밋
