# Approval Manager Implementation Plan (Phase 11)

> Historical plan: its two-person approval state is superseded by
> `docs/superpowers/specs/2026-07-31-solo-product-approval-design.md`.

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `crates/approval-manager` — verdict/PreviewModel → 순수 승인 상태머신. `ApprovalRequest`가 `state()`/`can_execute()`를 원시 데이터에서 파생하고 `approve`/`reject`로 변경.

**Architecture:** 저장 state enum 없음 — `state()`는 (verdict, required_approvals, approvals, rejection)에서 순수 계산. DB/API/실행/twilight 없음.

**Tech Stack:** Rust edition 2021 stable, serde, thiserror, serde_json(dev), policy-engine·discord-model·preview.

## Global Constraints
> ⚠️ **주석 금지**. 결정적(state 순수 파생).
- 의존: `approval-manager → {policy-engine, discord-model, preview}`. **DB/API/executor/bot-runtime/twilight/ai-gateway/web 의존 금지.**
- 완료 게이트: build/test/clippy(-D warnings)/fmt. **Phase 완료 후 `git push origin main`.**

---

### Task 1: approval-manager crate (상태머신 전체 + 테스트)

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/approval-manager/Cargo.toml`, `src/{lib.rs, request.rs}`

**Interfaces:**
- Produces: `ApprovalRequest`, `ApprovalState`, `ApprovalError`, `Rejection`, `new/from_preview/state/can_execute/approve/reject`.

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/approval-manager"` 추가.

Create `crates/approval-manager/Cargo.toml`:
```toml
[package]
name = "approval-manager"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
discord-model = { path = "../discord-model" }
policy-engine = { path = "../policy-engine" }
preview = { path = "../preview" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/approval-manager/src/lib.rs`:
```rust
pub mod request;

pub use request::{ApprovalError, ApprovalRequest, ApprovalState, Rejection};
```

- [ ] **Step 2: request.rs 테스트 작성 (상태머신 시나리오)**

Create `crates/approval-manager/src/request.rs` (테스트 먼저):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_approval_flow() {
        let mut r = ApprovalRequest::new(Verdict::RequireApproval, UserId(1));
        assert_eq!(r.required_approvals, 1);
        assert_eq!(r.state(), ApprovalState::PendingApproval);
        assert!(!r.can_execute());
        assert!(r.approve(UserId(2)).is_ok());
        assert_eq!(r.state(), ApprovalState::Approved);
        assert!(r.can_execute());
    }

    #[test]
    fn allow_is_ready_to_execute() {
        let mut r = ApprovalRequest::new(Verdict::Allow, UserId(1));
        assert_eq!(r.required_approvals, 0);
        assert_eq!(r.state(), ApprovalState::ReadyToExecute);
        assert!(r.can_execute());
        assert_eq!(r.approve(UserId(2)), Err(ApprovalError::NotRequired));
    }

    #[test]
    fn deny_is_blocked() {
        let mut r = ApprovalRequest::new(Verdict::Deny, UserId(1));
        assert_eq!(r.state(), ApprovalState::Blocked);
        assert!(!r.can_execute());
        assert_eq!(r.approve(UserId(2)), Err(ApprovalError::Blocked));
    }

    #[test]
    fn second_approval_needs_two_distinct_users() {
        let mut r = ApprovalRequest::new(Verdict::RequireSecondApproval, UserId(1));
        assert_eq!(r.required_approvals, 2);
        assert_eq!(r.state(), ApprovalState::PendingApproval);
        assert!(r.approve(UserId(2)).is_ok());
        assert_eq!(r.state(), ApprovalState::PendingSecondApproval);
        assert!(!r.can_execute());
        assert_eq!(r.approve(UserId(2)), Err(ApprovalError::DuplicateApproval));
        assert!(r.approve(UserId(3)).is_ok());
        assert_eq!(r.state(), ApprovalState::Approved);
        assert!(r.can_execute());
    }

    #[test]
    fn rejection_blocks_execution() {
        let mut r = ApprovalRequest::new(Verdict::RequireApproval, UserId(1));
        assert!(r.reject(UserId(2), "not safe").is_ok());
        assert_eq!(r.state(), ApprovalState::Rejected);
        assert!(!r.can_execute());
        assert_eq!(r.approve(UserId(3)), Err(ApprovalError::AlreadyRejected));
    }

    #[test]
    fn from_preview_reads_verdict() {
        let preview = PreviewModel {
            title: "t".to_string(),
            verdict: Verdict::RequireApproval,
            approval_required: true,
            blocked: false,
            changes: vec![],
            access_changes: vec![],
            policy_findings: vec![],
            warnings: vec![],
            deferred: vec![],
        };
        let r = ApprovalRequest::from_preview(&preview, UserId(1));
        assert_eq!(r.verdict, Verdict::RequireApproval);
        assert_eq!(r.required_approvals, 1);
    }

    #[test]
    fn approval_request_serde_roundtrip() {
        let mut r = ApprovalRequest::new(Verdict::RequireSecondApproval, UserId(1));
        r.approve(UserId(2)).unwrap();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<ApprovalRequest>(&json).unwrap(), r);
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p approval-manager` → FAIL(ApprovalRequest 미구현).

- [ ] **Step 4: request.rs 구현**

`request.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

use discord_model::UserId;
use policy_engine::Verdict;
use preview::PreviewModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Blocked,
    ReadyToExecute,
    PendingApproval,
    PendingSecondApproval,
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ApprovalError {
    #[error("request is blocked")]
    Blocked,
    #[error("request already rejected")]
    AlreadyRejected,
    #[error("request already approved")]
    AlreadyApproved,
    #[error("user already approved")]
    DuplicateApproval,
    #[error("no approval required")]
    NotRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejection {
    pub by: UserId,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub verdict: Verdict,
    pub requester: UserId,
    pub required_approvals: usize,
    pub approvals: Vec<UserId>,
    pub rejection: Option<Rejection>,
}

fn required_for(verdict: Verdict) -> usize {
    match verdict {
        Verdict::Allow | Verdict::Warn | Verdict::Deny => 0,
        Verdict::RequireApproval => 1,
        Verdict::RequireSecondApproval => 2,
    }
}

impl ApprovalRequest {
    pub fn new(verdict: Verdict, requester: UserId) -> Self {
        Self {
            verdict,
            requester,
            required_approvals: required_for(verdict),
            approvals: Vec::new(),
            rejection: None,
        }
    }

    pub fn from_preview(preview: &PreviewModel, requester: UserId) -> Self {
        Self::new(preview.verdict, requester)
    }

    pub fn state(&self) -> ApprovalState {
        if self.verdict == Verdict::Deny {
            return ApprovalState::Blocked;
        }
        if self.rejection.is_some() {
            return ApprovalState::Rejected;
        }
        if self.required_approvals == 0 {
            return ApprovalState::ReadyToExecute;
        }
        if self.approvals.len() >= self.required_approvals {
            return ApprovalState::Approved;
        }
        if self.required_approvals == 2 && self.approvals.len() == 1 {
            return ApprovalState::PendingSecondApproval;
        }
        ApprovalState::PendingApproval
    }

    pub fn can_execute(&self) -> bool {
        matches!(self.state(), ApprovalState::ReadyToExecute | ApprovalState::Approved)
    }

    pub fn approve(&mut self, user: UserId) -> Result<(), ApprovalError> {
        self.gate()?;
        if self.approvals.contains(&user) {
            return Err(ApprovalError::DuplicateApproval);
        }
        self.approvals.push(user);
        Ok(())
    }

    pub fn reject(&mut self, user: UserId, reason: &str) -> Result<(), ApprovalError> {
        self.gate()?;
        self.rejection = Some(Rejection { by: user, reason: reason.to_string() });
        Ok(())
    }

    fn gate(&self) -> Result<(), ApprovalError> {
        match self.state() {
            ApprovalState::Blocked => Err(ApprovalError::Blocked),
            ApprovalState::Rejected => Err(ApprovalError::AlreadyRejected),
            ApprovalState::ReadyToExecute => Err(ApprovalError::NotRequired),
            ApprovalState::Approved => Err(ApprovalError::AlreadyApproved),
            ApprovalState::PendingApproval | ApprovalState::PendingSecondApproval => Ok(()),
        }
    }
}
```

- [ ] **Step 5: 통과 확인** — `cargo test -p approval-manager` → 7개 통과.

- [ ] **Step 6: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 실제 출력대로 보고.

- [ ] **Step 7: 커밋 + push + 보고**
```bash
git add -A
git commit -m "feat(approval-manager): add pure approval state machine"
git push origin main
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] approval-manager: ApprovalRequest/ApprovalState(6)/ApprovalError(5)/Rejection + new/from_preview/state/can_execute/approve/reject
- [ ] 파생 상태(저장 enum 없음), state() 우선순위(Deny 최우선), approve/reject는 Pending일 때만
- [ ] **크라운 주얼**: RequireApproval→approve→Approved / Deny→Blocked→Err(Blocked) / SecondApproval 2명+중복 Err / Rejection→Err(AlreadyRejected)
- [ ] 의존 방향(상위 금지)·주석 없음·**main push**
