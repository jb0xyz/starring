# Preview Model Implementation Plan (Phase 10)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `crates/preview` — `build_preview(title, &DiffResult, &OperationGraph, &PolicyDecision, &VirtualApplyResult, &AccessMatrix before, &AccessMatrix after) -> PreviewModel`. 코어 결과를 승인용 UI-중립 데이터로 결정론적 합성.

**Architecture:** 순수 합성 캡스톤. graph→changes, before/after matrix→access_changes, policy→verdict/findings, apply→warnings, diff→deferred. 재계산/재평가/실행/DB/AI 없음.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), 코어 crate 다수 의존.

## Global Constraints
> ⚠️ **주석 금지**. 결정적(changes=graph 순서, access_changes=정렬 순서).
- 의존: `preview → {diff-engine, operation-graph, policy-engine, virtual-apply, simulator, desired-compiler, desired-state, discord-model}`. **상위 실행/DB/AI(ai-gateway/executor/bot-runtime/db/approval/web) 의존 금지.**
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋. **Phase 완료 후 `git push origin main`.**

---

### Task 1: 스캐폴드 + PreviewModel 타입 + build_preview

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/preview/Cargo.toml`, `src/{lib.rs, model.rs, build.rs}`

**Interfaces:**
- Produces: `PreviewModel`, `PreviewChange`, `AccessChange`, `PreviewSeverity`, `PreviewChangeKind`, `build_preview(...)`.

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/preview"` 추가.

Create `crates/preview/Cargo.toml`:
```toml
[package]
name = "preview"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
discord-model = { path = "../discord-model" }
desired-state = { path = "../desired-state" }
desired-compiler = { path = "../desired-compiler" }
diff-engine = { path = "../diff-engine" }
operation-graph = { path = "../operation-graph" }
policy-engine = { path = "../policy-engine" }
virtual-apply = { path = "../virtual-apply" }
simulator = { path = "../simulator" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/preview/src/lib.rs`:
```rust
pub mod build;
pub mod model;

pub use build::build_preview;
pub use model::{AccessChange, PreviewChange, PreviewChangeKind, PreviewModel, PreviewSeverity};
```

Create `crates/preview/src/model.rs`:
```rust
use serde::{Deserialize, Serialize};

use policy_engine::{Finding, Verdict};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewSeverity {
    Info,
    Notice,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewChangeKind {
    RoleCreate,
    RoleUpdate,
    RoleDelete,
    ChannelCreate,
    ChannelUpdate,
    ChannelDelete,
    OverwriteCreate,
    OverwriteUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewChange {
    pub kind: PreviewChangeKind,
    pub target: String,
    pub severity: PreviewSeverity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessChange {
    pub subject: String,
    pub channel: String,
    pub before_can_view: bool,
    pub after_can_view: bool,
    pub before_can_send: bool,
    pub after_can_send: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewModel {
    pub title: String,
    pub verdict: Verdict,
    pub approval_required: bool,
    pub blocked: bool,
    pub changes: Vec<PreviewChange>,
    pub access_changes: Vec<AccessChange>,
    pub policy_findings: Vec<Finding>,
    pub warnings: Vec<String>,
    pub deferred: Vec<String>,
}
```

- [ ] **Step 2: build 테스트 작성**

Create `crates/preview/src/build.rs` (테스트 먼저):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{Guild, GuildId, GuildState, Permissions, UserId};
    use simulator::AccessCell;
    use std::collections::BTreeMap;

    fn apply_result(warnings: Vec<String>) -> VirtualApplyResult {
        VirtualApplyResult {
            after: GuildState {
                guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) },
                roles: vec![],
                channels: vec![],
                members: vec![],
            },
            applied: vec![],
            synthetic_roles: BTreeMap::new(),
            synthetic_channels: BTreeMap::new(),
            warnings,
        }
    }
    fn cell(subject: &str, channel: &str, v: bool, s: bool) -> AccessCell {
        AccessCell { subject: subject.to_string(), channel: channel.to_string(), can_view: v, can_send: s }
    }

    #[test]
    fn delete_role_is_warning_create_is_info() {
        let d = change_of(&Operation::DeleteRole { key: ResourceKey("vip".to_string()) });
        assert_eq!(d.kind, PreviewChangeKind::RoleDelete);
        assert_eq!(d.severity, PreviewSeverity::Warning);
        let c = change_of(&Operation::CreateRole { key: ResourceKey("vip".to_string()), name: Some("VIP".to_string()), permissions: None });
        assert_eq!(c.severity, PreviewSeverity::Info);
        assert_eq!(c.target, "VIP");
    }

    #[test]
    fn everyone_overwrite_is_notice() {
        let c = change_of(&Operation::CreateOverwrite {
            channel: ResourceKey("general".to_string()),
            target: NormalizedTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::empty(),
        });
        assert_eq!(c.severity, PreviewSeverity::Notice);
        assert_eq!(c.target, "general / @everyone");
    }

    #[test]
    fn access_changes_diff_union_and_unchanged() {
        let before = AccessMatrix { cells: vec![cell("new", "general", true, false)] };
        let after = AccessMatrix { cells: vec![cell("new", "general", false, false), cell("verified", "general", true, true)] };
        let changes = access_changes(&before, &after);
        assert_eq!(changes.len(), 2);
        let v = changes.iter().find(|c| c.subject == "verified").unwrap();
        assert!(!v.before_can_view && v.after_can_view && v.after_can_send);
        let n = changes.iter().find(|c| c.subject == "new").unwrap();
        assert!(n.before_can_view && !n.after_can_view);

        let same = AccessMatrix { cells: vec![cell("new", "general", true, false)] };
        assert!(access_changes(&same, &same).is_empty());
    }

    #[test]
    fn build_preview_derives_verdict_flags() {
        let policy = PolicyDecision { verdict: Verdict::RequireApproval, findings: vec![] };
        let p = build_preview("t", &DiffResult::default(), &OperationGraph::default(), &policy,
            &apply_result(vec!["w".to_string()]), &AccessMatrix::default(), &AccessMatrix::default());
        assert!(p.approval_required);
        assert!(!p.blocked);
        assert_eq!(p.warnings, vec!["w".to_string()]);
        assert!(serde_json::to_string(&p).is_ok());

        let denied = PolicyDecision { verdict: Verdict::Deny, findings: vec![] };
        let p2 = build_preview("t", &DiffResult::default(), &OperationGraph::default(), &denied,
            &apply_result(vec![]), &AccessMatrix::default(), &AccessMatrix::default());
        assert!(p2.blocked);
        assert!(!p2.approval_required);
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p preview` → FAIL(build_preview 미구현).

- [ ] **Step 4: build.rs 구현**

`build.rs` 테스트 위에:
```rust
use std::collections::{BTreeSet, HashMap};

use desired_compiler::NormalizedTarget;
use desired_state::ResourceKey;
use diff_engine::DiffResult;
use operation_graph::{Operation, OperationGraph};
use policy_engine::{PolicyDecision, Verdict};
use simulator::AccessMatrix;
use virtual_apply::VirtualApplyResult;

use crate::model::{
    AccessChange, PreviewChange, PreviewChangeKind, PreviewModel, PreviewSeverity,
};

pub fn build_preview(
    title: &str,
    diff: &DiffResult,
    graph: &OperationGraph,
    policy: &PolicyDecision,
    apply: &VirtualApplyResult,
    before: &AccessMatrix,
    after: &AccessMatrix,
) -> PreviewModel {
    let verdict = policy.verdict;
    let approval_required =
        matches!(verdict, Verdict::RequireApproval | Verdict::RequireSecondApproval);
    let blocked = verdict == Verdict::Deny;

    let changes = graph.nodes.iter().map(|node| change_of(&node.operation)).collect();
    let access_changes = access_changes(before, after);
    let deferred = diff
        .deferred
        .iter()
        .map(|item| format!("{}:{}", item.kind, item.key.0))
        .collect();

    PreviewModel {
        title: title.to_string(),
        verdict,
        approval_required,
        blocked,
        changes,
        access_changes,
        policy_findings: policy.findings.clone(),
        warnings: apply.warnings.clone(),
        deferred,
    }
}

fn change_of(op: &Operation) -> PreviewChange {
    match op {
        Operation::CreateRole { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::RoleCreate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::UpdateRole { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::RoleUpdate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::DeleteRole { key } => PreviewChange {
            kind: PreviewChangeKind::RoleDelete,
            target: key.0.clone(),
            severity: PreviewSeverity::Warning,
        },
        Operation::CreateChannel { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::ChannelCreate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::UpdateChannel { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::ChannelUpdate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::DeleteChannel { key } => PreviewChange {
            kind: PreviewChangeKind::ChannelDelete,
            target: key.0.clone(),
            severity: PreviewSeverity::Warning,
        },
        Operation::CreateOverwrite { channel, target, .. } => PreviewChange {
            kind: PreviewChangeKind::OverwriteCreate,
            target: format!("{} / {}", channel.0, target_label(target)),
            severity: overwrite_severity(target),
        },
        Operation::UpdateOverwrite { channel, target, .. } => PreviewChange {
            kind: PreviewChangeKind::OverwriteUpdate,
            target: format!("{} / {}", channel.0, target_label(target)),
            severity: overwrite_severity(target),
        },
    }
}

fn label(name: &Option<String>, key: &ResourceKey) -> String {
    name.clone().unwrap_or_else(|| key.0.clone())
}

fn target_label(target: &NormalizedTarget) -> String {
    match target {
        NormalizedTarget::Everyone => "@everyone".to_string(),
        NormalizedTarget::Role(key) => format!("role:{}", key.0),
        NormalizedTarget::Member(id) => format!("member:{id}"),
    }
}

fn overwrite_severity(target: &NormalizedTarget) -> PreviewSeverity {
    match target {
        NormalizedTarget::Everyone => PreviewSeverity::Notice,
        _ => PreviewSeverity::Info,
    }
}

fn access_changes(before: &AccessMatrix, after: &AccessMatrix) -> Vec<AccessChange> {
    let before_map: HashMap<(&str, &str), (bool, bool)> = before
        .cells
        .iter()
        .map(|c| ((c.subject.as_str(), c.channel.as_str()), (c.can_view, c.can_send)))
        .collect();
    let after_map: HashMap<(&str, &str), (bool, bool)> = after
        .cells
        .iter()
        .map(|c| ((c.subject.as_str(), c.channel.as_str()), (c.can_view, c.can_send)))
        .collect();
    let keys: BTreeSet<(&str, &str)> =
        before_map.keys().chain(after_map.keys()).copied().collect();
    let mut changes = Vec::new();
    for (subject, channel) in keys {
        let b = before_map.get(&(subject, channel)).copied().unwrap_or((false, false));
        let a = after_map.get(&(subject, channel)).copied().unwrap_or((false, false));
        if b != a {
            changes.push(AccessChange {
                subject: subject.to_string(),
                channel: channel.to_string(),
                before_can_view: b.0,
                after_can_view: a.0,
                before_can_send: b.1,
                after_can_send: a.1,
            });
        }
    }
    changes
}
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p preview && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(preview): add PreviewModel and deterministic build_preview"
```

- [ ] **Step 6: Task 보고**

---

### Task 2: 크라운 주얼 (full pipeline → preview) + 최종 게이트

**Files:**
- Create: `crates/preview/tests/pipeline_scenario.rs`

**Interfaces:**
- Produces: DesiredState → compile → diff → graph → policy → virtual-apply → simulator → build_preview end-to-end.

- [ ] **Step 1: pipeline_scenario.rs 작성**

일반 `[dependencies]`(desired-compiler/desired-state/diff-engine/operation-graph/policy-engine/virtual-apply/simulator/discord-model)는 통합 테스트에서 그대로 `use` 가능 → 새 dev-dep 불필요.

Create `crates/preview/tests/pipeline_scenario.rs`:
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
use policy_engine::{PolicyEngine, Verdict};
use preview::{build_preview, PreviewChangeKind, PreviewSeverity};
use simulator::{access_matrix, SubjectSpec};
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
fn full_pipeline_to_preview() {
    let before = before_guild();
    let normalized = compile(&desired()).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    let policy = PolicyEngine::with_default_rules().evaluate(&graph);
    let applied = apply(&before, &graph, &normalized, &InMemoryMatchResolver::new(&before)).unwrap();
    let after = &applied.after;

    let verified_id = applied.synthetic_roles[&ResourceKey("verified".to_string())];
    let before_subjects = vec![SubjectSpec { name: "new_member".to_string(), roles: vec![] }];
    let after_subjects = vec![
        SubjectSpec { name: "new_member".to_string(), roles: vec![] },
        SubjectSpec { name: "verified_member".to_string(), roles: vec![verified_id] },
    ];
    let before_matrix = access_matrix(&before, &before_subjects);
    let after_matrix = access_matrix(after, &after_subjects);

    let p = build_preview("인증 시스템 설정", &diff_result, &graph, &policy, &applied, &before_matrix, &after_matrix);

    assert_eq!(p.verdict, Verdict::RequireApproval);
    assert!(p.approval_required);
    assert!(!p.blocked);
    assert!(p.changes.iter().any(|c| c.kind == PreviewChangeKind::RoleCreate && c.target == "Verified"));
    assert!(p.changes.iter().any(|c| c.severity == PreviewSeverity::Notice && c.target.contains("@everyone")));
    assert!(p.access_changes.iter().any(|a| a.subject == "new_member" && a.channel == "general" && a.before_can_view && !a.after_can_view));
    assert!(p.access_changes.iter().any(|a| a.subject == "verified_member" && a.channel == "general" && !a.before_can_view && a.after_can_view));
    assert!(p.policy_findings.iter().any(|f| f.rule_id == "everyone-change"));
}
```

- [ ] **Step 2: 통과 확인** — `cargo test -p preview --test pipeline_scenario`. re-export 누락 시 lib.rs 보완.

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
git commit -m "test(preview): add full pipeline scenario to approval preview"
git push origin main
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] preview: PreviewModel(9필드)/PreviewChange/AccessChange/PreviewSeverity/PreviewChangeKind + build_preview
- [ ] verdict 파생(approval_required/blocked), severity(delete=Warning/@everyone=Notice/else=Info), access_changes 합집합 정렬 diff, deferred/warnings 매핑, Finding 재사용
- [ ] **크라운 주얼**: full pipeline → preview (verdict=RequireApproval, changes에 RoleCreate(Verified)+@everyone Notice, access_changes에 new true→false/verified false→true, findings에 everyone-change)
- [ ] 의존 방향(상위 금지)·주석 없음·Task별 커밋·**main push**
