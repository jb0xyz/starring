# Executor Core Implementation Plan (Phase 12b)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `crates/executor-core` — `Executor<A: DiscordAdapter>::execute(&ApprovedExecutionRequest).await -> Result<JobResult, ExecutorError>`. 승인된 OperationGraph를 topo 순서로 adapter 호출 실행, fail-fast, rollback 캡처. Mock으로 결정론 테스트.

**Architecture:** native async fn in trait(#[allow], generic 정적 디스패치). 해소는 resource-resolution 재사용. 실세계 Discord는 DiscordAdapter 뒤 → MockDiscordAdapter로 테스트. tokio/async-trait 없음(테스트는 futures::block_on).

**Tech Stack:** Rust edition 2021 stable, serde, thiserror, 코어 crate; dev: futures, serde_json, policy-engine.

## Global Constraints
> ⚠️ **주석 금지**. 결정적(Mock 카운터·topo). `#[allow(async_fn_in_trait)]`만 예외 allow.
- 의존: `executor-core → {operation-graph, approval-manager, resource-resolution, desired-compiler, desired-state, diff-engine, discord-model}`. **twilight/tokio/async-trait/NATS/DB 금지.**
- 스펙: `docs/superpowers/specs/2026-07-09-executor-core-design.md`.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋. **Phase 완료 후 `git push origin main`.**

---

### Task 1: 타입 + DiscordAdapter trait + MockDiscordAdapter

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/executor-core/Cargo.toml`, `src/{lib.rs, adapter.rs, result.rs, request.rs, mock.rs}`

**Interfaces:**
- Produces: `DiscordAdapter`, `RoleSpec`, `ChannelSpec`, `AdapterError`, `AdapterErrorKind`, `StepOutcome`, `StepResult`, `CreatedResource`, `JobStatus`, `JobResult`, `RollbackAction`, `ApprovedExecutionRequest`, `ExecutorError`, `MockDiscordAdapter`, `AdapterCall`.

- [ ] **Step 1: 워크스페이스 + Cargo + lib.rs**

Root `Cargo.toml` members에 `"crates/executor-core"` 추가.

Create `crates/executor-core/Cargo.toml`:
```toml
[package]
name = "executor-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }
desired-state = { path = "../desired-state" }
desired-compiler = { path = "../desired-compiler" }
diff-engine = { path = "../diff-engine" }
operation-graph = { path = "../operation-graph" }
approval-manager = { path = "../approval-manager" }
resource-resolution = { path = "../resource-resolution" }

[dev-dependencies]
futures = "0.3"
serde_json = { workspace = true }
policy-engine = { path = "../policy-engine" }
```

Create `crates/executor-core/src/lib.rs`:
```rust
pub mod adapter;
pub mod execute;
pub mod mock;
pub mod request;
pub mod result;

pub use adapter::{AdapterError, AdapterErrorKind, ChannelSpec, DiscordAdapter, RoleSpec};
pub use execute::Executor;
pub use mock::{AdapterCall, MockDiscordAdapter};
pub use request::{ApprovedExecutionRequest, ExecutorError};
pub use result::{
    CreatedResource, JobResult, JobStatus, RollbackAction, StepOutcome, StepResult,
};
```

- [ ] **Step 2: adapter.rs**

Create `crates/executor-core/src/adapter.rs`:
```rust
use serde::{Deserialize, Serialize};

use discord_model::{ChannelId, ChannelType, GuildId, OverwriteTarget, Permissions, RoleId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSpec {
    pub name: Option<String>,
    pub permissions: Option<Permissions>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSpec {
    pub name: Option<String>,
    pub channel_type: Option<ChannelType>,
    pub parent_id: Option<ChannelId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKind {
    RateLimited,
    Timeout,
    Network,
    ServerError,
    Forbidden,
    MissingPermissions,
    RoleHierarchy,
    NotFound,
    BadRequest,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterError {
    pub kind: AdapterErrorKind,
    pub message: String,
}

impl AdapterError {
    pub fn new(kind: AdapterErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            AdapterErrorKind::RateLimited
                | AdapterErrorKind::Timeout
                | AdapterErrorKind::Network
                | AdapterErrorKind::ServerError
        )
    }
}

#[allow(async_fn_in_trait)]
pub trait DiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError>;
    async fn update_role(
        &self,
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    ) -> Result<(), AdapterError>;
    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError>;
    async fn create_channel(
        &self,
        guild: GuildId,
        spec: ChannelSpec,
    ) -> Result<ChannelId, AdapterError>;
    async fn update_channel(
        &self,
        guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    ) -> Result<(), AdapterError>;
    async fn delete_channel(&self, guild: GuildId, id: ChannelId) -> Result<(), AdapterError>;
    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError>;
}
```

- [ ] **Step 3: result.rs**

Create `crates/executor-core/src/result.rs`:
```rust
use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;
use discord_model::{Channel, ChannelId, OverwriteTarget, PermissionOverwrite, Role, RoleId};
use operation_graph::OpId;

use crate::adapter::AdapterError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOutcome {
    Success,
    FailedRetryable(AdapterError),
    FailedFatal(AdapterError),
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreatedResource {
    Role { key: ResourceKey, id: RoleId },
    Channel { key: ResourceKey, id: ChannelId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollbackAction {
    DeleteRole { id: RoleId },
    RestoreRole { id: RoleId, before: Role },
    RecreateRole { before: Role },
    DeleteChannel { id: ChannelId },
    RestoreChannel { id: ChannelId, before: Channel },
    RecreateChannel { before: Channel },
    RestoreOverwrite {
        channel: ChannelId,
        target: OverwriteTarget,
        before: Option<PermissionOverwrite>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    pub op_id: OpId,
    pub outcome: StepOutcome,
    pub created: Option<CreatedResource>,
    pub rollback: Option<RollbackAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobResult {
    pub status: JobStatus,
    pub steps: Vec<StepResult>,
}
```

- [ ] **Step 4: request.rs**

Create `crates/executor-core/src/request.rs`:
```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

use approval_manager::ApprovalRequest;
use desired_compiler::NormalizedDesiredState;
use discord_model::{GuildId, GuildState, UserId};
use operation_graph::OperationGraph;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedExecutionRequest {
    pub operation_graph: OperationGraph,
    pub normalized: NormalizedDesiredState,
    pub approval: ApprovalRequest,
    pub snapshot: GuildState,
    pub guild_id: GuildId,
    pub requested_by: UserId,
    pub approved_by: Vec<UserId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ExecutorError {
    #[error("request is not approved for execution")]
    NotApproved,
    #[error("operation graph has a cycle")]
    GraphCycle,
}
```

- [ ] **Step 5: mock.rs 테스트 작성**

Create `crates/executor-core/src/mock.rs` (테스트 먼저):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterErrorKind;
    use futures::executor::block_on;

    #[test]
    fn create_role_returns_fake_id_and_records() {
        let mock = MockDiscordAdapter::new();
        let id = block_on(mock.create_role(
            GuildId(1),
            RoleSpec { name: Some("VIP".to_string()), permissions: None },
        ))
        .unwrap();
        assert_eq!(id, RoleId(900_000));
        assert_eq!(mock.calls().len(), 1);
        assert!(matches!(mock.calls()[0], AdapterCall::CreateRole { .. }));
    }

    #[test]
    fn fail_on_triggers_at_call_number() {
        let mock = MockDiscordAdapter::with_failure(
            1,
            AdapterError::new(AdapterErrorKind::RateLimited, "rl"),
        );
        let r = block_on(mock.create_role(GuildId(1), RoleSpec { name: None, permissions: None }));
        assert_eq!(r.unwrap_err().kind, AdapterErrorKind::RateLimited);
    }
}
```

- [ ] **Step 6: 실패 확인** — `cargo test -p executor-core` → FAIL(MockDiscordAdapter 미구현).

- [ ] **Step 7: mock.rs 구현**

`mock.rs` 테스트 위에:
```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId};

use crate::adapter::{AdapterError, ChannelSpec, DiscordAdapter, RoleSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterCall {
    CreateRole { guild: GuildId, spec: RoleSpec },
    UpdateRole { guild: GuildId, id: RoleId, spec: RoleSpec },
    DeleteRole { guild: GuildId, id: RoleId },
    CreateChannel { guild: GuildId, spec: ChannelSpec },
    UpdateChannel { guild: GuildId, id: ChannelId, spec: ChannelSpec },
    DeleteChannel { guild: GuildId, id: ChannelId },
    UpsertOverwrite {
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
}

pub struct MockDiscordAdapter {
    next_id: AtomicU64,
    calls: Mutex<Vec<AdapterCall>>,
    fail_on: Option<(usize, AdapterError)>,
}

impl MockDiscordAdapter {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(900_000),
            calls: Mutex::new(Vec::new()),
            fail_on: None,
        }
    }

    pub fn with_failure(call_number: usize, error: AdapterError) -> Self {
        Self {
            next_id: AtomicU64::new(900_000),
            calls: Mutex::new(Vec::new()),
            fail_on: Some((call_number, error)),
        }
    }

    pub fn calls(&self) -> Vec<AdapterCall> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: AdapterCall) -> usize {
        let mut calls = self.calls.lock().unwrap();
        calls.push(call);
        calls.len()
    }

    fn check_fail(&self, call_number: usize) -> Result<(), AdapterError> {
        if let Some((n, err)) = &self.fail_on {
            if *n == call_number {
                return Err(err.clone());
            }
        }
        Ok(())
    }

    fn next_role(&self) -> RoleId {
        RoleId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    fn next_channel(&self) -> ChannelId {
        ChannelId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for MockDiscordAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordAdapter for MockDiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError> {
        let n = self.record(AdapterCall::CreateRole { guild, spec });
        self.check_fail(n)?;
        Ok(self.next_role())
    }

    async fn update_role(
        &self,
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    ) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::UpdateRole { guild, id, spec });
        self.check_fail(n)
    }

    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::DeleteRole { guild, id });
        self.check_fail(n)
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: ChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        let n = self.record(AdapterCall::CreateChannel { guild, spec });
        self.check_fail(n)?;
        Ok(self.next_channel())
    }

    async fn update_channel(
        &self,
        guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    ) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::UpdateChannel { guild, id, spec });
        self.check_fail(n)
    }

    async fn delete_channel(&self, guild: GuildId, id: ChannelId) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::DeleteChannel { guild, id });
        self.check_fail(n)
    }

    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        let n = self.record(AdapterCall::UpsertOverwrite {
            guild,
            channel,
            target,
            allow,
            deny,
        });
        self.check_fail(n)
    }
}
```

- [ ] **Step 8: 통과 + 커밋**
```bash
cargo test -p executor-core && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(executor-core): add DiscordAdapter trait, result types, and mock"
```

- [ ] **Step 9: Task 보고**

---

### Task 2: Executor + execute() + 시나리오 테스트

**Files:**
- Create: `crates/executor-core/src/execute.rs`, `crates/executor-core/tests/executor_scenario.rs`

**Interfaces:**
- Consumes: 모든 Task 1 타입 + resource-resolution + 파이프라인(compile/diff/compile_operations).
- Produces: `Executor<A>`, `execute()`.

- [ ] **Step 1: execute.rs 구현**

Create `crates/executor-core/src/execute.rs`:
```rust
use diff_engine::{InMemoryMatchResolver, ResourceResolver};
use discord_model::{GuildId, GuildState};
use operation_graph::{OpId, Operation};
use resource_resolution::{ResolutionError, ResourceResolutionContext};

use crate::adapter::{AdapterError, AdapterErrorKind, ChannelSpec, DiscordAdapter, RoleSpec};
use crate::request::{ApprovedExecutionRequest, ExecutorError};
use crate::result::{
    CreatedResource, JobResult, JobStatus, RollbackAction, StepOutcome, StepResult,
};

pub struct Executor<A: DiscordAdapter> {
    adapter: A,
}

impl<A: DiscordAdapter> Executor<A> {
    pub fn new(adapter: A) -> Self {
        Self { adapter }
    }

    pub fn adapter(&self) -> &A {
        &self.adapter
    }

    pub async fn execute(
        &self,
        request: &ApprovedExecutionRequest,
    ) -> Result<JobResult, ExecutorError> {
        if !request.approval.can_execute() {
            return Err(ExecutorError::NotApproved);
        }
        let order = request
            .operation_graph
            .topological_order()
            .map_err(|_| ExecutorError::GraphCycle)?;

        let resolver = InMemoryMatchResolver::new(&request.snapshot);
        let mut ctx =
            ResourceResolutionContext::new(&request.normalized, &resolver, request.guild_id);

        let mut steps = Vec::new();
        let mut stopped = false;
        for id in order {
            let operation = match request.operation_graph.nodes.iter().find(|n| n.id == id) {
                Some(node) => &node.operation,
                None => continue,
            };
            if stopped {
                steps.push(StepResult {
                    op_id: id,
                    outcome: StepOutcome::Skipped,
                    created: None,
                    rollback: None,
                });
                continue;
            }
            let step = self
                .run_op(id, operation, &mut ctx, &request.snapshot, request.guild_id)
                .await;
            if !matches!(step.outcome, StepOutcome::Success) {
                stopped = true;
            }
            steps.push(step);
        }
        let status = if stopped {
            JobStatus::Failed
        } else {
            JobStatus::Succeeded
        };
        Ok(JobResult { status, steps })
    }

    async fn run_op<R: ResourceResolver>(
        &self,
        op_id: OpId,
        op: &Operation,
        ctx: &mut ResourceResolutionContext<'_, R>,
        snapshot: &GuildState,
        guild: GuildId,
    ) -> StepResult {
        let (outcome, created, rollback) = match op {
            Operation::CreateRole {
                key,
                name,
                permissions,
            } => {
                let spec = RoleSpec {
                    name: name.clone(),
                    permissions: *permissions,
                };
                match self.adapter.create_role(guild, spec).await {
                    Ok(id) => {
                        ctx.bind_role(key.clone(), id);
                        (
                            StepOutcome::Success,
                            Some(CreatedResource::Role {
                                key: key.clone(),
                                id,
                            }),
                            Some(RollbackAction::DeleteRole { id }),
                        )
                    }
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::UpdateRole {
                key,
                name,
                permissions,
            } => {
                let id = match ctx.resolve_role_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.roles.iter().find(|r| r.id == id).cloned();
                let spec = RoleSpec {
                    name: name.clone(),
                    permissions: *permissions,
                };
                match self.adapter.update_role(guild, id, spec).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RestoreRole { id, before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::DeleteRole { key } => {
                let id = match ctx.resolve_role_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.roles.iter().find(|r| r.id == id).cloned();
                match self.adapter.delete_role(guild, id).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RecreateRole { before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::CreateChannel {
                key,
                name,
                channel_type,
                parent,
            } => {
                let parent_id = match parent {
                    Some(pk) => match ctx.resolve_channel_key(pk) {
                        Ok(id) => Some(id),
                        Err(e) => return resolution_step(op_id, e),
                    },
                    None => None,
                };
                let spec = ChannelSpec {
                    name: name.clone(),
                    channel_type: *channel_type,
                    parent_id,
                };
                match self.adapter.create_channel(guild, spec).await {
                    Ok(id) => {
                        ctx.bind_channel(key.clone(), id);
                        (
                            StepOutcome::Success,
                            Some(CreatedResource::Channel {
                                key: key.clone(),
                                id,
                            }),
                            Some(RollbackAction::DeleteChannel { id }),
                        )
                    }
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::UpdateChannel {
                key,
                name,
                channel_type,
            } => {
                let id = match ctx.resolve_channel_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.channels.iter().find(|c| c.id == id).cloned();
                let spec = ChannelSpec {
                    name: name.clone(),
                    channel_type: *channel_type,
                    parent_id: None,
                };
                match self.adapter.update_channel(guild, id, spec).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RestoreChannel { id, before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
            Operation::DeleteChannel { key } => {
                let id = match ctx.resolve_channel_key(key) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot.channels.iter().find(|c| c.id == id).cloned();
                match self.adapter.delete_channel(guild, id).await {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        before.map(|b| RollbackAction::RecreateChannel { before: b }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
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
                let channel_id = match ctx.resolve_channel_key(channel) {
                    Ok(id) => id,
                    Err(e) => return resolution_step(op_id, e),
                };
                let ow_target = match ctx.resolve_target(target) {
                    Ok(t) => t,
                    Err(e) => return resolution_step(op_id, e),
                };
                let before = snapshot
                    .channels
                    .iter()
                    .find(|c| c.id == channel_id)
                    .and_then(|c| c.overwrites.iter().find(|o| o.target == ow_target).cloned());
                match self
                    .adapter
                    .upsert_overwrite(guild, channel_id, ow_target, *allow, *deny)
                    .await
                {
                    Ok(()) => (
                        StepOutcome::Success,
                        None,
                        Some(RollbackAction::RestoreOverwrite {
                            channel: channel_id,
                            target: ow_target,
                            before,
                        }),
                    ),
                    Err(e) => (fail_outcome(e), None, None),
                }
            }
        };
        StepResult {
            op_id,
            outcome,
            created,
            rollback,
        }
    }
}

fn fail_outcome(e: AdapterError) -> StepOutcome {
    if e.is_retryable() {
        StepOutcome::FailedRetryable(e)
    } else {
        StepOutcome::FailedFatal(e)
    }
}

fn resolution_step(op_id: OpId, e: ResolutionError) -> StepResult {
    StepResult {
        op_id,
        outcome: StepOutcome::FailedFatal(AdapterError::new(
            AdapterErrorKind::Unknown,
            format!("unresolved: {e}"),
        )),
        created: None,
        rollback: None,
    }
}
```

- [ ] **Step 2: tests/executor_scenario.rs — 3축 테스트**

Create `crates/executor-core/tests/executor_scenario.rs`:
```rust
use std::collections::BTreeMap;

use approval_manager::ApprovalRequest;
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
use executor_core::{
    AdapterCall, AdapterError, AdapterErrorKind, ApprovedExecutionRequest, CreatedResource,
    Executor, ExecutorError, JobStatus, MockDiscordAdapter, RollbackAction, StepOutcome,
};
use futures::executor::block_on;
use operation_graph::compile_operations;
use policy_engine::Verdict;

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

fn request(verdict: Verdict) -> ApprovedExecutionRequest {
    let before = before_guild();
    let normalized = compile(&desired()).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval: ApprovalRequest::new(verdict, UserId(1)),
        snapshot: before,
        guild_id: GuildId(1),
        requested_by: UserId(1),
        approved_by: vec![UserId(1)],
    }
}

#[test]
fn success_executes_all_ops_and_threads_created_id() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();

    assert_eq!(result.status, JobStatus::Succeeded);
    assert!(result.steps.iter().all(|s| matches!(s.outcome, StepOutcome::Success)));

    let created_role = result.steps.iter().find_map(|s| match &s.created {
        Some(CreatedResource::Role { id, .. }) => Some(*id),
        _ => None,
    });
    assert_eq!(created_role, Some(RoleId(900_000)));

    let create_step = result.steps.iter().find(|s| matches!(s.created, Some(CreatedResource::Role { .. }))).unwrap();
    assert_eq!(create_step.rollback, Some(RollbackAction::DeleteRole { id: RoleId(900_000) }));

    let calls = executor.adapter().calls();
    assert!(matches!(calls.first(), Some(AdapterCall::CreateRole { .. })));
    assert!(calls.iter().any(|c| matches!(c, AdapterCall::UpsertOverwrite { target: OverwriteTarget::Role(RoleId(900_000)), .. })));
}

#[test]
fn fail_fast_stops_and_skips_rest() {
    let executor = Executor::new(MockDiscordAdapter::with_failure(
        2,
        AdapterError::new(AdapterErrorKind::MissingPermissions, "no perms"),
    ));
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();

    assert_eq!(result.status, JobStatus::Failed);
    assert!(matches!(result.steps[0].outcome, StepOutcome::Success));
    assert!(matches!(result.steps[1].outcome, StepOutcome::FailedFatal(_)));
    assert!(result.steps[2..].iter().all(|s| matches!(s.outcome, StepOutcome::Skipped)));
    assert_eq!(executor.adapter().calls().len(), 2);
}

#[test]
fn retryable_failure_also_stops() {
    let executor = Executor::new(MockDiscordAdapter::with_failure(
        1,
        AdapterError::new(AdapterErrorKind::RateLimited, "rl"),
    ));
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();
    assert_eq!(result.status, JobStatus::Failed);
    assert!(matches!(result.steps[0].outcome, StepOutcome::FailedRetryable(_)));
}

#[test]
fn not_approved_refuses_without_calls() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let err = block_on(executor.execute(&request(Verdict::Deny))).unwrap_err();
    assert_eq!(err, ExecutorError::NotApproved);
    assert_eq!(executor.adapter().calls().len(), 0);
}

#[test]
fn job_result_serde_roundtrips() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    assert_eq!(serde_json::from_str::<executor_core::JobResult>(&json).unwrap(), result);
}
```

- [ ] **Step 3: 통과 확인** — `cargo test -p executor-core`. re-export 누락 시 lib.rs 보완.

- [ ] **Step 4: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 실제 출력대로 보고.

- [ ] **Step 5: 커밋 + push + 보고**
```bash
git add -A
git commit -m "feat(executor-core): add Executor with fail-fast execution and rollback capture"
git push origin main
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] executor-core: DiscordAdapter trait(#[allow(async_fn_in_trait)]) + RoleSpec/ChannelSpec + AdapterError(is_retryable) + StepOutcome/StepResult/CreatedResource/JobStatus/JobResult/RollbackAction + ApprovedExecutionRequest/ExecutorError + MockDiscordAdapter/AdapterCall + Executor
- [ ] execute(): pre-flight(can_execute/topo) → topo 실행 → op별 adapter 호출 + 해소(resource-resolution) + created 바인딩 + rollback 캡처 → fail-fast 전체중단 → Skipped
- [ ] **테스트 3축**: 성공(전 op Success·created id 스레딩·콜 시퀀스) / fail-fast(fatal 중단+Skipped, retryable 중단) / 거부(NotApproved·콜 0) + serde
- [ ] tokio/async-trait 없음, 테스트 block_on, 의존 방향·주석 없음·Task별 커밋·**main push**
