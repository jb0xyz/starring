use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use chrono::{DateTime, Duration, Utc};
use discord_model::{GuildId, UserId};
use serde::{Deserialize, Serialize};

use crate::id::{ActivationRequestId, ApplyAttemptId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationTarget {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub content_hash: RuleSetContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub approver: UserId,
    pub approved_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejection {
    pub rejected_at: DateTime<Utc>,
    pub rejected_by: UserId,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionKind {
    Activated,
    AlreadyActive,
    CrashRecovered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub applied_at: DateTime<Utc>,
    pub applied_by: UserId,
    pub kind: CompletionKind,
    pub notices: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedActive {
    pub version: RuleSetVersionId,
    pub content_hash: RuleSetContentHash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyFailureKind {
    TargetMissing,
    TargetCorrupt,
    Environment,
    NotReady,
    Activation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyErrorRecord {
    pub kind: ApplyFailureKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationRequestState {
    Pending,
    Approved,
    Applying,
    Applied,
    Rejected,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationRequest {
    pub id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub required_approvals: u32,
    pub approvals: Vec<Approval>,
    pub state: ActivationRequestState,
    pub rejection: Option<Rejection>,
    pub apply_attempt_id: Option<ApplyAttemptId>,
    pub apply_attempt_no: u64,
    pub apply_lease_until: Option<DateTime<Utc>>,
    pub last_apply_error: Option<ApplyErrorRecord>,
    pub observed_active: Option<ObservedActive>,
    pub completion: Option<Completion>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateActivationRequest {
    pub id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub required_approvals: u32,
    pub ttl: Duration,
    pub observed_active: Option<ObservedActive>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimDecision {
    Claimed,
    InProgress {
        blocking_request_id: ActivationRequestId,
        lease_until: DateTime<Utc>,
        lease_expired: bool,
    },
    AlreadyApplied,
    NotApproved,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalDecisionError {
    #[error("self approval is forbidden")]
    SelfApprovalForbidden,
    #[error("approval already exists")]
    DuplicateApproval,
    #[error("request is not pending")]
    NotPending,
    #[error("request is expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RejectionDecisionError {
    #[error("request is not pending")]
    NotPending,
    #[error("request is expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("apply attempt number overflow")]
    AttemptOverflow,
}

impl ActivationRequest {
    pub fn create(input: CreateActivationRequest, now: DateTime<Utc>) -> Result<Self, String> {
        if input.required_approvals == 0 {
            return Err("required approvals must be at least one".to_string());
        }
        if input.ttl <= Duration::zero() {
            return Err("request ttl must be positive".to_string());
        }
        let expires_at = now
            .checked_add_signed(input.ttl)
            .ok_or_else(|| "request expiry overflow".to_string())?;
        Ok(Self {
            id: input.id,
            target: input.target,
            requester: input.requester,
            required_approvals: input.required_approvals,
            approvals: Vec::new(),
            state: ActivationRequestState::Pending,
            rejection: None,
            apply_attempt_id: None,
            apply_attempt_no: 0,
            apply_lease_until: None,
            last_apply_error: None,
            observed_active: input.observed_active,
            completion: None,
            created_at: now,
            expires_at,
        })
    }

    pub fn expire_if_due(&mut self, now: DateTime<Utc>) -> bool {
        if matches!(
            self.state,
            ActivationRequestState::Pending | ActivationRequestState::Approved
        ) && self.expires_at <= now
        {
            self.state = ActivationRequestState::Expired;
            true
        } else {
            false
        }
    }

    pub fn approve_at(
        &mut self,
        approver: UserId,
        now: DateTime<Utc>,
    ) -> Result<(), ApprovalDecisionError> {
        if self.expire_if_due(now) {
            return Err(ApprovalDecisionError::Expired);
        }
        if self.state != ActivationRequestState::Pending {
            return Err(ApprovalDecisionError::NotPending);
        }
        if approver == self.requester {
            return Err(ApprovalDecisionError::SelfApprovalForbidden);
        }
        if self
            .approvals
            .iter()
            .any(|approval| approval.approver == approver)
        {
            return Err(ApprovalDecisionError::DuplicateApproval);
        }
        self.approvals.push(Approval {
            approver,
            approved_at: now,
        });
        self.approvals.sort_by_key(|approval| approval.approver);
        if u32::try_from(self.approvals.len()).unwrap_or(u32::MAX) >= self.required_approvals {
            self.state = ActivationRequestState::Approved;
        }
        Ok(())
    }

    pub fn reject_at(
        &mut self,
        rejected_by: UserId,
        reason: String,
        now: DateTime<Utc>,
    ) -> Result<(), RejectionDecisionError> {
        if self.expire_if_due(now) {
            return Err(RejectionDecisionError::Expired);
        }
        if self.state != ActivationRequestState::Pending {
            return Err(RejectionDecisionError::NotPending);
        }
        self.state = ActivationRequestState::Rejected;
        self.rejection = Some(Rejection {
            rejected_at: now,
            rejected_by,
            reason,
        });
        Ok(())
    }

    pub fn claim_apply_at(
        &mut self,
        attempt_id: ApplyAttemptId,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimDecision, TransitionError> {
        if self.expire_if_due(now) {
            return Ok(ClaimDecision::Expired);
        }
        match self.state {
            ActivationRequestState::Approved => {
                self.begin_attempt(attempt_id, lease_until)?;
                Ok(ClaimDecision::Claimed)
            }
            ActivationRequestState::Applying => Ok(self.in_progress(now)),
            ActivationRequestState::Applied => Ok(ClaimDecision::AlreadyApplied),
            ActivationRequestState::Expired => Ok(ClaimDecision::Expired),
            ActivationRequestState::Pending | ActivationRequestState::Rejected => {
                Ok(ClaimDecision::NotApproved)
            }
        }
    }

    pub fn claim_resume_at(
        &mut self,
        attempt_id: ApplyAttemptId,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimDecision, TransitionError> {
        match self.state {
            ActivationRequestState::Applying => {
                if self.apply_lease_until.is_some_and(|lease| lease > now) {
                    return Ok(self.in_progress(now));
                }
                self.begin_attempt(attempt_id, lease_until)?;
                Ok(ClaimDecision::Claimed)
            }
            ActivationRequestState::Applied => Ok(ClaimDecision::AlreadyApplied),
            ActivationRequestState::Expired => Ok(ClaimDecision::Expired),
            ActivationRequestState::Pending
            | ActivationRequestState::Approved
            | ActivationRequestState::Rejected => Ok(ClaimDecision::NotApproved),
        }
    }

    pub fn renew_lease_at(
        &mut self,
        attempt_id: &ApplyAttemptId,
        lease_until: DateTime<Utc>,
    ) -> bool {
        if self.state != ActivationRequestState::Applying
            || self.apply_attempt_id.as_ref() != Some(attempt_id)
        {
            return false;
        }
        self.apply_lease_until = Some(lease_until);
        true
    }

    pub fn complete_at(&mut self, attempt_id: &ApplyAttemptId, completion: Completion) -> bool {
        if self.state != ActivationRequestState::Applying
            || self.apply_attempt_id.as_ref() != Some(attempt_id)
        {
            return false;
        }
        self.state = ActivationRequestState::Applied;
        self.apply_attempt_id = None;
        self.apply_lease_until = None;
        self.last_apply_error = None;
        self.completion = Some(completion);
        true
    }

    pub fn release_at(&mut self, attempt_id: &ApplyAttemptId, error: ApplyErrorRecord) -> bool {
        if self.state != ActivationRequestState::Applying
            || self.apply_attempt_id.as_ref() != Some(attempt_id)
        {
            return false;
        }
        self.state = ActivationRequestState::Approved;
        self.apply_attempt_id = None;
        self.apply_lease_until = None;
        self.last_apply_error = Some(error);
        true
    }

    pub fn bookkeep_at(&mut self, completion: Completion) -> bool {
        if self.state != ActivationRequestState::Applying {
            return false;
        }
        self.state = ActivationRequestState::Applied;
        self.apply_attempt_id = None;
        self.apply_lease_until = None;
        self.last_apply_error = None;
        self.completion = Some(completion);
        true
    }

    fn begin_attempt(
        &mut self,
        attempt_id: ApplyAttemptId,
        lease_until: DateTime<Utc>,
    ) -> Result<(), TransitionError> {
        self.apply_attempt_no = self
            .apply_attempt_no
            .checked_add(1)
            .ok_or(TransitionError::AttemptOverflow)?;
        self.state = ActivationRequestState::Applying;
        self.apply_attempt_id = Some(attempt_id);
        self.apply_lease_until = Some(lease_until);
        Ok(())
    }

    fn in_progress(&self, now: DateTime<Utc>) -> ClaimDecision {
        let lease_until = self.apply_lease_until.unwrap_or(now);
        ClaimDecision::InProgress {
            blocking_request_id: self.id.clone(),
            lease_until,
            lease_expired: lease_until <= now,
        }
    }
}
