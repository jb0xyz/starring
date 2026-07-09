# Automatic Rollback Execution Plan (Phase 15)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** executor-core에 opt-in `execute_with_rollback` 추가 — forward 실패 시 성공 step의 RollbackAction을 역순 실행해 원복, 결과를 `JobRun`으로 기록. **execute() 불변, bot-runtime/twilight 무변경, live 없음.**

**Architecture:** execute_with_rollback = execute() → 실패면 rollback(성공 step 역순, adapter 재사용) → JobRun. RestoreOverwrite{None}은 Skipped(delete_overwrite 미지원).

## Global Constraints
> ⚠️ **주석 금지**. **execute() 절대 수정 금지**(기존 테스트 무변경). rollback 실패는 기록(panic 아님).
- 스펙: `docs/superpowers/specs/2026-07-10-automatic-rollback-design.md`.
- 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋. 완료 후 `git push origin main`.

---

### Task 1: rollback 타입 + Mock 다중실패

**Files:**
- Modify: `crates/executor-core/src/result.rs`, `src/lib.rs`, `src/mock.rs`

- [ ] **Step 1: result.rs — 타입 추가**

`crates/executor-core/src/result.rs` 끝에 추가:
```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackStatus {
    NotRequired,
    Succeeded,
    Partial,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollbackOutcome {
    Undone,
    Failed(AdapterError),
    Skipped { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackStepResult {
    pub source_op_id: OpId,
    pub action: RollbackAction,
    pub outcome: RollbackOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackReport {
    pub status: RollbackStatus,
    pub steps: Vec<RollbackStepResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRun {
    pub job: JobResult,
    pub rollback: RollbackReport,
}
```
(OpId/RollbackAction/JobResult/AdapterError는 result.rs에 이미 임포트됨.)

- [ ] **Step 2: lib.rs — export 추가**

`crates/executor-core/src/lib.rs`의 `pub use result::{...}`에 신규 타입 추가:
```rust
pub use result::{
    CreatedResource, JobResult, JobRun, JobStatus, RollbackAction, RollbackOutcome, RollbackReport,
    RollbackStatus, RollbackStepResult, StepOutcome, StepResult,
};
```

- [ ] **Step 3: mock.rs — fail_on을 Vec로 + with_failures**

`crates/executor-core/src/mock.rs` 수정:
- 필드: `fail_on: Option<(usize, AdapterError)>,` → `fail_on: Vec<(usize, AdapterError)>,`
- `new()`: `fail_on: None,` → `fail_on: Vec::new(),`
- `with_failure`: 본문을 `fail_on: vec![(call_number, error)],`로 (시그니처 유지)
- `with_failures` 추가 (with_failure 아래):
```rust
    pub fn with_failures(failures: Vec<(usize, AdapterError)>) -> Self {
        Self {
            next_id: AtomicU64::new(900_000),
            calls: Mutex::new(Vec::new()),
            fail_on: failures,
        }
    }
```
- `check_fail` 교체:
```rust
    fn check_fail(&self, call_number: usize) -> Result<(), AdapterError> {
        for (n, err) in &self.fail_on {
            if *n == call_number {
                return Err(err.clone());
            }
        }
        Ok(())
    }
```

`mock.rs` 테스트 모듈에 추가:
```rust
    #[test]
    fn with_failures_fails_multiple_calls() {
        let mock = MockDiscordAdapter::with_failures(vec![
            (1, AdapterError::new(AdapterErrorKind::RateLimited, "a")),
            (2, AdapterError::new(AdapterErrorKind::Forbidden, "b")),
        ]);
        assert!(block_on(mock.create_role(GuildId(1), RoleSpec { name: None, permissions: None })).is_err());
        assert!(block_on(mock.delete_role(GuildId(1), RoleId(5))).is_err());
        assert!(block_on(mock.delete_role(GuildId(1), RoleId(6))).is_ok());
    }
```

- [ ] **Step 4: 게이트 + 커밋**
```bash
cargo test -p executor-core && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(executor-core): add rollback report types and mock multi-failure"
```

- [ ] **Step 5: Task 보고**

---

### Task 2: execute_with_rollback + 테스트

**Files:**
- Modify: `crates/executor-core/src/execute.rs`, `crates/executor-core/tests/executor_scenario.rs`

- [ ] **Step 1: execute.rs — import 확장**

`crates/executor-core/src/execute.rs`의 `use crate::result::{...}`를 교체:
```rust
use crate::result::{
    CreatedResource, JobResult, JobRun, JobStatus, RollbackAction, RollbackOutcome, RollbackReport,
    RollbackStatus, RollbackStepResult, StepOutcome, StepResult,
};
```

- [ ] **Step 2: execute.rs — execute_with_rollback/rollback/run_rollback 추가**

`impl<A: DiscordAdapter> Executor<A>` 안(`execute` 아래, `run_op` 위 어디든)에 추가:
```rust
    pub async fn execute_with_rollback(
        &self,
        request: &ApprovedExecutionRequest,
    ) -> Result<JobRun, ExecutorError> {
        let job = self.execute(request).await?;
        let rollback = if matches!(job.status, JobStatus::Succeeded) {
            RollbackReport { status: RollbackStatus::NotRequired, steps: Vec::new() }
        } else {
            self.rollback(&job, request.guild_id).await
        };
        Ok(JobRun { job, rollback })
    }

    async fn rollback(&self, job: &JobResult, guild: GuildId) -> RollbackReport {
        let mut steps = Vec::new();
        for step in job.steps.iter().rev() {
            if !matches!(step.outcome, StepOutcome::Success) {
                continue;
            }
            let action = match &step.rollback {
                Some(action) => action,
                None => continue,
            };
            let outcome = self.run_rollback(guild, action).await;
            steps.push(RollbackStepResult {
                source_op_id: step.op_id,
                action: action.clone(),
                outcome,
            });
        }
        RollbackReport { status: rollback_status(&steps), steps }
    }

    async fn run_rollback(&self, guild: GuildId, action: &RollbackAction) -> RollbackOutcome {
        let result = match action {
            RollbackAction::DeleteRole { id } => self.adapter.delete_role(guild, *id).await,
            RollbackAction::RestoreRole { id, before } => {
                self.adapter
                    .update_role(
                        guild,
                        *id,
                        RoleSpec {
                            name: Some(before.name.clone()),
                            permissions: Some(before.permissions),
                        },
                    )
                    .await
            }
            RollbackAction::RecreateRole { before } => self
                .adapter
                .create_role(
                    guild,
                    RoleSpec {
                        name: Some(before.name.clone()),
                        permissions: Some(before.permissions),
                    },
                )
                .await
                .map(|_| ()),
            RollbackAction::DeleteChannel { id } => self.adapter.delete_channel(guild, *id).await,
            RollbackAction::RestoreChannel { id, before } => {
                self.adapter
                    .update_channel(
                        guild,
                        *id,
                        ChannelSpec {
                            name: Some(before.name.clone()),
                            channel_type: Some(before.channel_type),
                            parent_id: before.parent_id,
                        },
                    )
                    .await
            }
            RollbackAction::RecreateChannel { before } => self
                .adapter
                .create_channel(
                    guild,
                    ChannelSpec {
                        name: Some(before.name.clone()),
                        channel_type: Some(before.channel_type),
                        parent_id: before.parent_id,
                    },
                )
                .await
                .map(|_| ()),
            RollbackAction::RestoreOverwrite { channel, target, before } => match before {
                Some(overwrite) => {
                    self.adapter
                        .upsert_overwrite(guild, *channel, *target, overwrite.allow, overwrite.deny)
                        .await
                }
                None => {
                    return RollbackOutcome::Skipped {
                        reason: "delete overwrite is not supported in Phase 15".to_string(),
                    }
                }
            },
        };
        match result {
            Ok(()) => RollbackOutcome::Undone,
            Err(error) => RollbackOutcome::Failed(error),
        }
    }
```

그리고 파일 하단의 free 함수 영역(`fn fail_outcome` 근처)에 추가:
```rust
fn rollback_status(steps: &[RollbackStepResult]) -> RollbackStatus {
    if steps.is_empty() {
        return RollbackStatus::NotRequired;
    }
    if steps.iter().all(|s| matches!(s.outcome, RollbackOutcome::Undone)) {
        return RollbackStatus::Succeeded;
    }
    if steps.iter().all(|s| matches!(s.outcome, RollbackOutcome::Failed(_))) {
        return RollbackStatus::Failed;
    }
    RollbackStatus::Partial
}
```

- [ ] **Step 3: execute.rs — 직접 rollback 단위 테스트 추가(private 접근)**

`execute.rs` 끝에 추가:
```rust
#[cfg(test)]
mod rollback_tests {
    use super::*;
    use crate::adapter::AdapterErrorKind;
    use crate::mock::MockDiscordAdapter;
    use discord_model::{ChannelId, OverwriteTarget, RoleId};

    fn failed_job(rollback: RollbackAction) -> JobResult {
        JobResult {
            status: JobStatus::Failed,
            steps: vec![StepResult {
                op_id: OpId(0),
                outcome: StepOutcome::Success,
                created: None,
                rollback: Some(rollback),
            }],
        }
    }

    #[test]
    fn created_overwrite_rollback_is_skipped() {
        let executor = Executor::new(MockDiscordAdapter::new());
        let job = failed_job(RollbackAction::RestoreOverwrite {
            channel: ChannelId(1),
            target: OverwriteTarget::Role(RoleId(2)),
            before: None,
        });
        let report = futures::executor::block_on(executor.rollback(&job, GuildId(1)));
        assert_eq!(report.status, RollbackStatus::Partial);
        assert!(matches!(report.steps[0].outcome, RollbackOutcome::Skipped { .. }));
        assert_eq!(executor.adapter().calls().len(), 0);
    }

    #[test]
    fn adapter_failure_is_recorded() {
        let executor = Executor::new(MockDiscordAdapter::with_failure(
            1,
            AdapterError::new(AdapterErrorKind::Forbidden, "no"),
        ));
        let job = failed_job(RollbackAction::DeleteRole { id: RoleId(900_000) });
        let report = futures::executor::block_on(executor.rollback(&job, GuildId(1)));
        assert_eq!(report.status, RollbackStatus::Failed);
        assert!(matches!(report.steps[0].outcome, RollbackOutcome::Failed(_)));
    }
}
```

- [ ] **Step 4: tests/executor_scenario.rs — execute_with_rollback 흐름**

`crates/executor-core/tests/executor_scenario.rs`의 executor_core import에 `RollbackStatus` 추가하고, 파일 끝에 테스트 추가:
```rust
#[test]
fn success_needs_no_rollback() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let run = block_on(executor.execute_with_rollback(&request(Verdict::Allow))).unwrap();
    assert_eq!(run.job.status, JobStatus::Succeeded);
    assert_eq!(run.rollback.status, RollbackStatus::NotRequired);
    assert!(run.rollback.steps.is_empty());
}

#[test]
fn failure_triggers_rollback_of_created_role() {
    let executor = Executor::new(MockDiscordAdapter::with_failure(
        2,
        AdapterError::new(AdapterErrorKind::MissingPermissions, "no"),
    ));
    let run = block_on(executor.execute_with_rollback(&request(Verdict::Allow))).unwrap();
    assert_eq!(run.job.status, JobStatus::Failed);
    assert_eq!(run.rollback.status, RollbackStatus::Succeeded);
    assert_eq!(run.rollback.steps.len(), 1);
    assert!(matches!(run.rollback.steps[0].action, RollbackAction::DeleteRole { .. }));
}

#[test]
fn not_approved_skips_rollback() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let err = block_on(executor.execute_with_rollback(&request(Verdict::Deny))).unwrap_err();
    assert_eq!(err, ExecutorError::NotApproved);
    assert_eq!(executor.adapter().calls().len(), 0);
}
```
> `use executor_core::{..., RollbackStatus, ...}` 추가(기존 import에). JobStatus/RollbackAction/Executor/ExecutorError/AdapterError/AdapterErrorKind/MockDiscordAdapter/block_on은 이미 있음.

- [ ] **Step 5: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. **기존 execute() 테스트 무변경 통과** + 신규 rollback 테스트. 총 테스트 실제 출력대로 보고.

- [ ] **Step 6: 커밋 + push + 보고**
```bash
git add -A
git commit -m "feat(executor-core): add opt-in execute_with_rollback with reverse-order best-effort rollback"
git push origin main
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] result: JobRun/RollbackReport/RollbackStatus/RollbackStepResult/RollbackOutcome + lib export
- [ ] execute_with_rollback(forward 실패→성공step 역순 rollback→JobRun), rollback_status 규칙(empty=NotRequired/all Undone=Succeeded/all Failed=Failed/else Partial)
- [ ] RestoreOverwrite{None}=Skipped(reason)·adapter 미호출. RollbackOutcome::Failed에 AdapterError
- [ ] Mock fail_on Vec+with_failures(with_failure 시그니처 유지, 카운트 forward+rollback 공유)
- [ ] **execute() 및 기존 테스트 무변경**·주석 없음·Task별 커밋·main push
