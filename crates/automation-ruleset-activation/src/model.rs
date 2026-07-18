use std::num::NonZeroU64;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use chrono::{DateTime, Duration, Utc};
use discord_model::{GuildId, UserId};
use resource_resolution::ApprovalBindingFingerprint;
use serde::{Deserialize, Serialize};

use crate::approval::{
    product_approval_context_digest_v1, ActivationApprovalContextV1, ActivationLinkStateV1,
    ProductApprovalContextV1,
};
use crate::id::{ActivationRequestId, ApplyAttemptId};
use crate::{ActivationDigest, ActivationPromotionId};

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
    pub approval_payload_digest: Option<ActivationDigest>,
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
    Superseded,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum SupersessionReasonV1 {
    ActiveBaselineDrift {
        expected: crate::ExpectedActiveBaselineV1,
        observed: crate::ExpectedActiveBaselineV1,
    },
    BindingDrift {
        expected_revision: NonZeroU64,
        observed_revision: NonZeroU64,
        expected_fingerprint: ApprovalBindingFingerprint,
        observed_fingerprint: Option<ApprovalBindingFingerprint>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivationTerminationV1 {
    Superseded {
        at: DateTime<Utc>,
        reason: SupersessionReasonV1,
    },
    Withdrawn {
        at: DateTime<Utc>,
        by: UserId,
        reason: String,
    },
}

impl ActivationTerminationV1 {
    fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Superseded { at, .. } | Self::Withdrawn { at, .. } => *at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationRequest {
    pub id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub required_approvals: u32,
    pub approval_context: ActivationApprovalContextV1,
    pub link_state: ActivationLinkStateV1,
    pub approvals: Vec<Approval>,
    pub state: ActivationRequestState,
    pub rejection: Option<Rejection>,
    pub apply_attempt_id: Option<ApplyAttemptId>,
    pub apply_attempt_no: u64,
    pub apply_lease_until: Option<DateTime<Utc>>,
    pub last_apply_error: Option<ApplyErrorRecord>,
    pub observed_active: Option<ObservedActive>,
    pub completion: Option<Completion>,
    pub termination: Option<ActivationTerminationV1>,
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
pub struct CreateProductActivationRequest {
    pub id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub context: ProductApprovalContextV1,
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
    Unlinked,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalDecisionError {
    #[error("self approval is forbidden")]
    SelfApprovalForbidden,
    #[error("approval already exists")]
    DuplicateApproval,
    #[error("product activation requires payload-bound approval")]
    BoundApprovalRequired,
    #[error("product activation request is not linked")]
    Unlinked,
    #[error("approval payload digest does not match")]
    PayloadMismatch,
    #[error("request is not pending")]
    NotPending,
    #[error("request is expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RejectionDecisionError {
    #[error("request is not pending")]
    NotPending,
    #[error("product activation request is not linked")]
    Unlinked,
    #[error("request is expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WithdrawDecisionError {
    #[error("product activation request is not linked")]
    Unlinked,
    #[error("activation request cannot be withdrawn from its current state")]
    InvalidState,
    #[error("activation request is expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("apply attempt number overflow")]
    AttemptOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkDecision {
    Linked,
    ExactReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkDecisionError {
    #[error("activation request is not product-authored")]
    NotProduct,
    #[error("activation link identity conflicts with the product request")]
    Conflict,
    #[error("activation request is not pending")]
    NotPending,
    #[error("activation request is expired")]
    Expired,
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
        let request = Self {
            id: input.id,
            target: input.target,
            requester: input.requester,
            required_approvals: input.required_approvals,
            approval_context: ActivationApprovalContextV1::LegacyManual,
            link_state: ActivationLinkStateV1::NotRequired,
            approvals: Vec::new(),
            state: ActivationRequestState::Pending,
            rejection: None,
            apply_attempt_id: None,
            apply_attempt_no: 0,
            apply_lease_until: None,
            last_apply_error: None,
            observed_active: input.observed_active,
            completion: None,
            termination: None,
            created_at: now,
            expires_at,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn create_product(
        input: CreateProductActivationRequest,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        if !input.context.policy.validate()
            || !input.context.binding.validate(input.target.guild_id)
            || product_approval_context_digest_v1(
                &input.id,
                &input.target,
                input.requester,
                &input.context,
            ) != input.context.approval_context_digest
        {
            return Err("product approval context is invalid".to_string());
        }
        let ttl_seconds = i64::try_from(input.context.policy.ttl_seconds.get())
            .map_err(|_| "request ttl overflow".to_string())?;
        let ttl =
            Duration::try_seconds(ttl_seconds).ok_or_else(|| "request ttl overflow".to_string())?;
        let expires_at = now
            .checked_add_signed(ttl)
            .ok_or_else(|| "request expiry overflow".to_string())?;
        let request = Self {
            id: input.id,
            target: input.target,
            requester: input.requester,
            required_approvals: input.context.policy.required_approvals.get(),
            observed_active: input.context.baseline.as_observed(),
            approval_context: ActivationApprovalContextV1::ProductAuthoring {
                context: Box::new(input.context),
            },
            link_state: ActivationLinkStateV1::Unlinked,
            approvals: Vec::new(),
            state: ActivationRequestState::Pending,
            rejection: None,
            apply_attempt_id: None,
            apply_attempt_no: 0,
            apply_lease_until: None,
            last_apply_error: None,
            completion: None,
            termination: None,
            created_at: now,
            expires_at,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.required_approvals == 0 || self.expires_at <= self.created_at {
            return Err("activation request policy or expiry is invalid".to_string());
        }
        if !self
            .approvals
            .windows(2)
            .all(|window| window[0].approver < window[1].approver)
            || self.approvals.iter().any(|approval| {
                approval.approver == self.requester
                    || approval.approved_at < self.created_at
                    || approval.approved_at >= self.expires_at
            })
        {
            return Err("activation approvals are invalid".to_string());
        }
        let approval_count = u32::try_from(self.approvals.len()).unwrap_or(u32::MAX);
        if self.rejection.as_ref().is_some_and(|rejection| {
            rejection.rejected_at < self.created_at || rejection.rejected_at >= self.expires_at
        }) || self
            .completion
            .as_ref()
            .is_some_and(|completion| completion.applied_at < self.created_at)
            || self
                .termination
                .as_ref()
                .is_some_and(|termination| termination.at() < self.created_at)
        {
            return Err("activation decision timestamps are invalid".to_string());
        }
        match &self.approval_context {
            ActivationApprovalContextV1::LegacyManual => {
                if self.link_state != ActivationLinkStateV1::NotRequired
                    || self
                        .approvals
                        .iter()
                        .any(|approval| approval.approval_payload_digest.is_some())
                {
                    return Err("legacy activation context is invalid".to_string());
                }
            }
            ActivationApprovalContextV1::ProductAuthoring { context } => {
                if !context.policy.validate()
                    || !context.binding.validate(self.target.guild_id)
                    || context.policy.required_approvals.get() != self.required_approvals
                    || context.baseline.as_observed() != self.observed_active
                    || product_approval_context_digest_v1(
                        &self.id,
                        &self.target,
                        self.requester,
                        context,
                    ) != context.approval_context_digest
                {
                    return Err("product activation context is invalid".to_string());
                }
                let ttl_seconds = i64::try_from(context.policy.ttl_seconds.get())
                    .map_err(|_| "product activation ttl is invalid".to_string())?;
                let ttl = Duration::try_seconds(ttl_seconds)
                    .ok_or_else(|| "product activation ttl is invalid".to_string())?;
                if self.expires_at - self.created_at != ttl
                    || self.approvals.iter().any(|approval| {
                        approval.approval_payload_digest.as_ref()
                            != Some(&context.approval_payload_digest)
                    })
                {
                    return Err("product activation approval evidence is invalid".to_string());
                }
                match self.link_state {
                    ActivationLinkStateV1::NotRequired => {
                        return Err("product activation link state is invalid".to_string())
                    }
                    ActivationLinkStateV1::Unlinked => {
                        if !matches!(
                            self.state,
                            ActivationRequestState::Pending | ActivationRequestState::Expired
                        ) || !self.approvals.is_empty()
                            || self.apply_attempt_no != 0
                        {
                            return Err("unlinked product activation is not inert".to_string());
                        }
                    }
                    ActivationLinkStateV1::Linked { linked_at } => {
                        if linked_at < self.created_at || linked_at >= self.expires_at {
                            return Err("product activation link timestamp is invalid".to_string());
                        }
                        if self
                            .approvals
                            .iter()
                            .any(|approval| approval.approved_at < linked_at)
                        {
                            return Err("product activation approval predates its link".to_string());
                        }
                    }
                }
            }
        }
        let applying_fields = self.apply_attempt_id.is_some() && self.apply_lease_until.is_some();
        if (self.state == ActivationRequestState::Applying) != applying_fields
            || (self.state != ActivationRequestState::Applying
                && (self.apply_attempt_id.is_some() || self.apply_lease_until.is_some()))
        {
            return Err("activation apply fields are invalid".to_string());
        }
        match self.state {
            ActivationRequestState::Pending => {
                if approval_count >= self.required_approvals
                    || self.rejection.is_some()
                    || self.completion.is_some()
                    || self.termination.is_some()
                {
                    return Err("pending activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Approved => {
                if approval_count < self.required_approvals
                    || self.rejection.is_some()
                    || self.completion.is_some()
                    || self.termination.is_some()
                {
                    return Err("approved activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Applying => {
                if approval_count < self.required_approvals
                    || self.rejection.is_some()
                    || self.completion.is_some()
                    || self.termination.is_some()
                {
                    return Err("applying activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Applied => {
                if approval_count < self.required_approvals
                    || self.completion.is_none()
                    || self.rejection.is_some()
                    || self.termination.is_some()
                {
                    return Err("applied activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Rejected => {
                if self.rejection.is_none()
                    || self.completion.is_some()
                    || self.termination.is_some()
                {
                    return Err("rejected activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Expired => {
                if self.rejection.is_some()
                    || self.completion.is_some()
                    || self.termination.is_some()
                {
                    return Err("expired activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Superseded => {
                if approval_count < self.required_approvals
                    || self.rejection.is_some()
                    || self.completion.is_some()
                    || !matches!(
                        self.termination,
                        Some(ActivationTerminationV1::Superseded { .. })
                    )
                    || !matches!(
                        self.approval_context,
                        ActivationApprovalContextV1::ProductAuthoring { .. }
                    )
                    || !matches!(self.link_state, ActivationLinkStateV1::Linked { .. })
                {
                    return Err("superseded activation state is invalid".to_string());
                }
            }
            ActivationRequestState::Withdrawn => {
                if self.rejection.is_some()
                    || self.completion.is_some()
                    || !matches!(
                        self.termination,
                        Some(ActivationTerminationV1::Withdrawn { .. })
                    )
                {
                    return Err("withdrawn activation state is invalid".to_string());
                }
            }
        }
        Ok(())
    }

    pub fn link_product_at(
        &mut self,
        promotion_id: &ActivationPromotionId,
        promotion_request_digest: &ActivationDigest,
        approval_context_digest: &ActivationDigest,
        now: DateTime<Utc>,
    ) -> Result<LinkDecision, LinkDecisionError> {
        let ActivationApprovalContextV1::ProductAuthoring { context } = &self.approval_context
        else {
            return Err(LinkDecisionError::NotProduct);
        };
        if &context.promotion_id != promotion_id
            || &context.promotion_request_digest != promotion_request_digest
            || &context.approval_context_digest != approval_context_digest
        {
            return Err(LinkDecisionError::Conflict);
        }
        if matches!(self.link_state, ActivationLinkStateV1::Linked { .. }) {
            self.expire_if_due(now);
            return Ok(LinkDecision::ExactReplay);
        }
        if self.expire_if_due(now) {
            return Err(LinkDecisionError::Expired);
        }
        match self.link_state {
            ActivationLinkStateV1::Linked { .. } => unreachable!(),
            ActivationLinkStateV1::NotRequired => Err(LinkDecisionError::NotProduct),
            ActivationLinkStateV1::Unlinked => {
                if self.state != ActivationRequestState::Pending || !self.approvals.is_empty() {
                    return Err(LinkDecisionError::NotPending);
                }
                self.link_state = ActivationLinkStateV1::Linked { linked_at: now };
                Ok(LinkDecision::Linked)
            }
        }
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
        if matches!(
            self.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) {
            return Err(ApprovalDecisionError::BoundApprovalRequired);
        }
        self.approve_with_digest_at(approver, None, now)
    }

    pub fn approve_bound_at(
        &mut self,
        approver: UserId,
        approval_payload_digest: &ActivationDigest,
        now: DateTime<Utc>,
    ) -> Result<(), ApprovalDecisionError> {
        if self.expire_if_due(now) {
            return Err(ApprovalDecisionError::Expired);
        }
        let ActivationApprovalContextV1::ProductAuthoring { context } = &self.approval_context
        else {
            return Err(ApprovalDecisionError::PayloadMismatch);
        };
        if !matches!(self.link_state, ActivationLinkStateV1::Linked { .. }) {
            return Err(ApprovalDecisionError::Unlinked);
        }
        if &context.approval_payload_digest != approval_payload_digest {
            return Err(ApprovalDecisionError::PayloadMismatch);
        }
        self.approve_with_digest_at(approver, Some(approval_payload_digest.clone()), now)
    }

    fn approve_with_digest_at(
        &mut self,
        approver: UserId,
        approval_payload_digest: Option<ActivationDigest>,
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
            approval_payload_digest,
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
        if matches!(
            self.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) && !matches!(self.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return Err(RejectionDecisionError::Unlinked);
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
        if matches!(
            self.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) && !matches!(self.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return Ok(ClaimDecision::Unlinked);
        }
        match self.state {
            ActivationRequestState::Approved => {
                self.begin_attempt(attempt_id, lease_until)?;
                Ok(ClaimDecision::Claimed)
            }
            ActivationRequestState::Applying => Ok(self.in_progress(now)),
            ActivationRequestState::Applied => Ok(ClaimDecision::AlreadyApplied),
            ActivationRequestState::Expired => Ok(ClaimDecision::Expired),
            ActivationRequestState::Pending
            | ActivationRequestState::Rejected
            | ActivationRequestState::Superseded
            | ActivationRequestState::Withdrawn => Ok(ClaimDecision::NotApproved),
        }
    }

    pub fn claim_resume_at(
        &mut self,
        attempt_id: ApplyAttemptId,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimDecision, TransitionError> {
        if matches!(
            self.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) && !matches!(self.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return Ok(ClaimDecision::Unlinked);
        }
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
            | ActivationRequestState::Rejected
            | ActivationRequestState::Superseded
            | ActivationRequestState::Withdrawn => Ok(ClaimDecision::NotApproved),
        }
    }

    pub fn supersede_at(
        &mut self,
        attempt_id: &ApplyAttemptId,
        reason: SupersessionReasonV1,
        now: DateTime<Utc>,
    ) -> bool {
        if self.state != ActivationRequestState::Applying
            || self.apply_attempt_id.as_ref() != Some(attempt_id)
            || !matches!(
                self.approval_context,
                ActivationApprovalContextV1::ProductAuthoring { .. }
            )
            || !matches!(self.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return false;
        }
        self.state = ActivationRequestState::Superseded;
        self.apply_attempt_id = None;
        self.apply_lease_until = None;
        self.last_apply_error = None;
        self.termination = Some(ActivationTerminationV1::Superseded { at: now, reason });
        true
    }

    pub fn withdraw_at(
        &mut self,
        by: UserId,
        reason: String,
        now: DateTime<Utc>,
    ) -> Result<(), WithdrawDecisionError> {
        if self.expire_if_due(now) {
            return Err(WithdrawDecisionError::Expired);
        }
        if matches!(
            self.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) && !matches!(self.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return Err(WithdrawDecisionError::Unlinked);
        }
        if !matches!(
            self.state,
            ActivationRequestState::Pending | ActivationRequestState::Approved
        ) {
            return Err(WithdrawDecisionError::InvalidState);
        }
        self.state = ActivationRequestState::Withdrawn;
        self.last_apply_error = None;
        self.termination = Some(ActivationTerminationV1::Withdrawn {
            at: now,
            by,
            reason,
        });
        Ok(())
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
