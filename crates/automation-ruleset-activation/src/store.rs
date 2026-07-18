use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use discord_model::{GuildId, UserId};

use crate::id::{ActivationDigest, ActivationPromotionId, ActivationRequestId, ApplyAttemptId};
use crate::model::{
    ActivationRequest, ActivationRequestState, ApplyErrorRecord, ApprovalDecisionError,
    ClaimDecision, Completion, CompletionKind, CreateActivationRequest,
    CreateProductActivationRequest, LinkDecisionError, RejectionDecisionError,
};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActivationStoreError {
    #[error("activation request already exists")]
    DuplicateRequest,
    #[error("activation request not found")]
    NotFound,
    #[error("invalid activation request: {0}")]
    InvalidRequest(String),
    #[error("activation store backend error: {0}")]
    Backend(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApproveError {
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
    #[error("activation request is not pending")]
    NotPending,
    #[error("activation request is expired")]
    Expired,
    #[error(transparent)]
    Store(#[from] ActivationStoreError),
}

impl From<ApprovalDecisionError> for ApproveError {
    fn from(error: ApprovalDecisionError) -> Self {
        match error {
            ApprovalDecisionError::SelfApprovalForbidden => Self::SelfApprovalForbidden,
            ApprovalDecisionError::DuplicateApproval => Self::DuplicateApproval,
            ApprovalDecisionError::BoundApprovalRequired => Self::BoundApprovalRequired,
            ApprovalDecisionError::Unlinked => Self::Unlinked,
            ApprovalDecisionError::PayloadMismatch => Self::PayloadMismatch,
            ApprovalDecisionError::NotPending => Self::NotPending,
            ApprovalDecisionError::Expired => Self::Expired,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RejectError {
    #[error("activation request is not pending")]
    NotPending,
    #[error("product activation request is not linked")]
    Unlinked,
    #[error("activation request is expired")]
    Expired,
    #[error(transparent)]
    Store(#[from] ActivationStoreError),
}

impl From<RejectionDecisionError> for RejectError {
    fn from(error: RejectionDecisionError) -> Self {
        match error {
            RejectionDecisionError::NotPending => Self::NotPending,
            RejectionDecisionError::Unlinked => Self::Unlinked,
            RejectionDecisionError::Expired => Self::Expired,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimOutcome {
    Claimed(Box<ActivationRequest>),
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

impl ClaimOutcome {
    fn from_decision(decision: ClaimDecision, request: ActivationRequest) -> Self {
        match decision {
            ClaimDecision::Claimed => Self::Claimed(Box::new(request)),
            ClaimDecision::InProgress {
                blocking_request_id,
                lease_until,
                lease_expired,
            } => Self::InProgress {
                blocking_request_id,
                lease_until,
                lease_expired,
            },
            ClaimDecision::AlreadyApplied => Self::AlreadyApplied,
            ClaimDecision::NotApproved => Self::NotApproved,
            ClaimDecision::Unlinked => Self::Unlinked,
            ClaimDecision::Expired => Self::Expired,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkProductActivation {
    pub promotion_id: ActivationPromotionId,
    pub promotion_request_digest: ActivationDigest,
    pub approval_context_digest: ActivationDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkProductError {
    #[error("activation request is not product-authored")]
    NotProduct,
    #[error("activation link identity conflicts with the product request")]
    Conflict,
    #[error("activation request is not pending")]
    NotPending,
    #[error("activation request is expired")]
    Expired,
    #[error(transparent)]
    Store(#[from] ActivationStoreError),
}

impl From<LinkDecisionError> for LinkProductError {
    fn from(error: LinkDecisionError) -> Self {
        match error {
            LinkDecisionError::NotProduct => Self::NotProduct,
            LinkDecisionError::Conflict => Self::Conflict,
            LinkDecisionError::NotPending => Self::NotPending,
            LinkDecisionError::Expired => Self::Expired,
        }
    }
}

pub trait ActivationClock: Clone + Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UtcActivationClock;

impl ActivationClock for UtcActivationClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone)]
pub struct ManualActivationClock {
    now: Arc<Mutex<DateTime<Utc>>>,
}

impl ManualActivationClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(Mutex::new(now)),
        }
    }

    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().unwrap() = now;
    }

    pub fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().unwrap();
        *now = now.checked_add_signed(duration).unwrap();
    }
}

impl ActivationClock for ManualActivationClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }
}

#[allow(async_fn_in_trait)]
pub trait ActivationRequestStore {
    async fn create(
        &self,
        request: CreateActivationRequest,
    ) -> Result<ActivationRequest, ActivationStoreError>;
    async fn create_product(
        &self,
        request: CreateProductActivationRequest,
    ) -> Result<ActivationRequest, ActivationStoreError>;
    async fn link_product(
        &self,
        request_id: &ActivationRequestId,
        link: LinkProductActivation,
    ) -> Result<ActivationRequest, LinkProductError>;
    async fn get(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<Option<ActivationRequest>, ActivationStoreError>;
    async fn approve(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
    ) -> Result<ActivationRequest, ApproveError>;
    async fn approve_bound(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
        approval_payload_digest: &ActivationDigest,
    ) -> Result<ActivationRequest, ApproveError>;
    async fn reject(
        &self,
        request_id: &ActivationRequestId,
        rejected_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, RejectError>;
    async fn claim_apply(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<ClaimOutcome, ActivationStoreError>;
    async fn claim_resume(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<ClaimOutcome, ActivationStoreError>;
    async fn renew_lease(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<bool, ActivationStoreError>;
    async fn complete_applied(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        applied_by: UserId,
        kind: CompletionKind,
        notices: Option<Vec<String>>,
    ) -> Result<bool, ActivationStoreError>;
    async fn release_to_approved(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        error: ApplyErrorRecord,
    ) -> Result<bool, ActivationStoreError>;
    async fn mark_expired(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<bool, ActivationStoreError>;
    async fn list_applying(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<ActivationRequest>, ActivationStoreError>;
    async fn bookkeep_applied(
        &self,
        request_id: &ActivationRequestId,
        applied_by: UserId,
    ) -> Result<bool, ActivationStoreError>;
}

pub struct InMemoryActivationRequestStore<C: ActivationClock = UtcActivationClock> {
    clock: C,
    requests: Mutex<BTreeMap<ActivationRequestId, ActivationRequest>>,
}

impl Default for InMemoryActivationRequestStore<UtcActivationClock> {
    fn default() -> Self {
        Self::with_clock(UtcActivationClock)
    }
}

impl<C: ActivationClock> InMemoryActivationRequestStore<C> {
    pub fn with_clock(clock: C) -> Self {
        Self {
            clock,
            requests: Mutex::new(BTreeMap::new()),
        }
    }

    fn lease_until(&self, seconds: i64) -> Result<DateTime<Utc>, ActivationStoreError> {
        self.clock
            .now()
            .checked_add_signed(Duration::seconds(seconds))
            .ok_or_else(|| ActivationStoreError::InvalidRequest("lease overflow".to_string()))
    }
}

impl<C: ActivationClock> ActivationRequestStore for InMemoryActivationRequestStore<C> {
    async fn create(
        &self,
        input: CreateActivationRequest,
    ) -> Result<ActivationRequest, ActivationStoreError> {
        let mut requests = self.requests.lock().unwrap();
        if requests.contains_key(&input.id) {
            return Err(ActivationStoreError::DuplicateRequest);
        }
        let request = ActivationRequest::create(input, self.clock.now())
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(request.id.clone(), request.clone());
        Ok(request)
    }

    async fn create_product(
        &self,
        input: CreateProductActivationRequest,
    ) -> Result<ActivationRequest, ActivationStoreError> {
        let mut requests = self.requests.lock().unwrap();
        if requests.contains_key(&input.id) {
            return Err(ActivationStoreError::DuplicateRequest);
        }
        let request = ActivationRequest::create_product(input, self.clock.now())
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(request.id.clone(), request.clone());
        Ok(request)
    }

    async fn link_product(
        &self,
        request_id: &ActivationRequestId,
        link: LinkProductActivation,
    ) -> Result<ActivationRequest, LinkProductError> {
        let mut requests = self.requests.lock().unwrap();
        let request = requests
            .get_mut(request_id)
            .ok_or(ActivationStoreError::NotFound)?;
        request.link_product_at(
            &link.promotion_id,
            &link.promotion_request_digest,
            &link.approval_context_digest,
            self.clock.now(),
        )?;
        request
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        Ok(request.clone())
    }

    async fn get(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<Option<ActivationRequest>, ActivationStoreError> {
        let mut requests = self.requests.lock().unwrap();
        if let Some(request) = requests.get_mut(request_id) {
            request.expire_if_due(self.clock.now());
            request
                .validate()
                .map_err(ActivationStoreError::InvalidRequest)?;
            return Ok(Some(request.clone()));
        }
        Ok(None)
    }

    async fn approve(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
    ) -> Result<ActivationRequest, ApproveError> {
        let mut requests = self.requests.lock().unwrap();
        let request = requests
            .get_mut(request_id)
            .ok_or(ActivationStoreError::NotFound)?;
        request.approve_at(approver, self.clock.now())?;
        request
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        Ok(request.clone())
    }

    async fn approve_bound(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
        approval_payload_digest: &ActivationDigest,
    ) -> Result<ActivationRequest, ApproveError> {
        let mut requests = self.requests.lock().unwrap();
        let request = requests
            .get_mut(request_id)
            .ok_or(ActivationStoreError::NotFound)?;
        request.approve_bound_at(approver, approval_payload_digest, self.clock.now())?;
        request
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        Ok(request.clone())
    }

    async fn reject(
        &self,
        request_id: &ActivationRequestId,
        rejected_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, RejectError> {
        let mut requests = self.requests.lock().unwrap();
        let request = requests
            .get_mut(request_id)
            .ok_or(ActivationStoreError::NotFound)?;
        request.reject_at(rejected_by, reason, self.clock.now())?;
        request
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        Ok(request.clone())
    }

    async fn claim_apply(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<ClaimOutcome, ActivationStoreError> {
        let now = self.clock.now();
        let lease_until = self.lease_until(lease_seconds)?;
        let mut requests = self.requests.lock().unwrap();
        let mut candidate = requests
            .get(request_id)
            .cloned()
            .ok_or(ActivationStoreError::NotFound)?;
        let decision = candidate
            .claim_apply_at(attempt_id, now, lease_until)
            .map_err(|error| ActivationStoreError::InvalidRequest(error.to_string()))?;
        if decision == ClaimDecision::Claimed {
            if let Some(blocking) = requests.values().find(|request| {
                request.id != candidate.id
                    && request.state == ActivationRequestState::Applying
                    && request.target.guild_id == candidate.target.guild_id
                    && request.target.ruleset_key == candidate.target.ruleset_key
            }) {
                let lease_until = blocking.apply_lease_until.unwrap_or(now);
                return Ok(ClaimOutcome::InProgress {
                    blocking_request_id: blocking.id.clone(),
                    lease_until,
                    lease_expired: lease_until <= now,
                });
            }
        }
        candidate
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(candidate.id.clone(), candidate.clone());
        Ok(ClaimOutcome::from_decision(decision, candidate))
    }

    async fn claim_resume(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<ClaimOutcome, ActivationStoreError> {
        let now = self.clock.now();
        let lease_until = self.lease_until(lease_seconds)?;
        let mut requests = self.requests.lock().unwrap();
        let request = requests
            .get_mut(request_id)
            .ok_or(ActivationStoreError::NotFound)?;
        let decision = request
            .claim_resume_at(attempt_id, now, lease_until)
            .map_err(|error| ActivationStoreError::InvalidRequest(error.to_string()))?;
        request
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        Ok(ClaimOutcome::from_decision(decision, request.clone()))
    }

    async fn renew_lease(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        lease_seconds: i64,
    ) -> Result<bool, ActivationStoreError> {
        let lease_until = self.lease_until(lease_seconds)?;
        let mut requests = self.requests.lock().unwrap();
        let Some(mut candidate) = requests.get(request_id).cloned() else {
            return Ok(false);
        };
        if !candidate.renew_lease_at(attempt_id, lease_until) {
            return Ok(false);
        }
        candidate
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(candidate.id.clone(), candidate);
        Ok(true)
    }

    async fn complete_applied(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        applied_by: UserId,
        kind: CompletionKind,
        notices: Option<Vec<String>>,
    ) -> Result<bool, ActivationStoreError> {
        let completion = Completion {
            applied_at: self.clock.now(),
            applied_by,
            kind,
            notices,
        };
        let mut requests = self.requests.lock().unwrap();
        let Some(mut candidate) = requests.get(request_id).cloned() else {
            return Ok(false);
        };
        if !candidate.complete_at(attempt_id, completion) {
            return Ok(false);
        }
        candidate
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(candidate.id.clone(), candidate);
        Ok(true)
    }

    async fn release_to_approved(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        error: ApplyErrorRecord,
    ) -> Result<bool, ActivationStoreError> {
        let mut requests = self.requests.lock().unwrap();
        let Some(mut candidate) = requests.get(request_id).cloned() else {
            return Ok(false);
        };
        if !candidate.release_at(attempt_id, error) {
            return Ok(false);
        }
        candidate
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(candidate.id.clone(), candidate);
        Ok(true)
    }

    async fn mark_expired(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<bool, ActivationStoreError> {
        let mut requests = self.requests.lock().unwrap();
        let Some(mut candidate) = requests.get(request_id).cloned() else {
            return Ok(false);
        };
        if !candidate.expire_if_due(self.clock.now()) {
            return Ok(false);
        }
        candidate
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(candidate.id.clone(), candidate);
        Ok(true)
    }

    async fn list_applying(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<ActivationRequest>, ActivationStoreError> {
        let requests = self.requests.lock().unwrap();
        let applying = requests
            .values()
            .filter(|request| {
                request.target.guild_id == guild_id
                    && request.state == ActivationRequestState::Applying
            })
            .cloned()
            .collect::<Vec<_>>();
        for request in &applying {
            request
                .validate()
                .map_err(ActivationStoreError::InvalidRequest)?;
        }
        Ok(applying)
    }

    async fn bookkeep_applied(
        &self,
        request_id: &ActivationRequestId,
        applied_by: UserId,
    ) -> Result<bool, ActivationStoreError> {
        let completion = Completion {
            applied_at: self.clock.now(),
            applied_by,
            kind: CompletionKind::CrashRecovered,
            notices: None,
        };
        let mut requests = self.requests.lock().unwrap();
        let Some(mut candidate) = requests.get(request_id).cloned() else {
            return Ok(false);
        };
        if !candidate.bookkeep_at(completion) {
            return Ok(false);
        }
        candidate
            .validate()
            .map_err(ActivationStoreError::InvalidRequest)?;
        requests.insert(candidate.id.clone(), candidate);
        Ok(true)
    }
}
