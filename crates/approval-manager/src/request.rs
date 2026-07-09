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
        matches!(
            self.state(),
            ApprovalState::ReadyToExecute | ApprovalState::Approved
        )
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
        self.rejection = Some(Rejection {
            by: user,
            reason: reason.to_string(),
        });
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
