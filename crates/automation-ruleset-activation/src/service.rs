use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration as StdDuration;

use automation_ruleset::{
    GuardedActivationOutcome, GuardedRuleSetActivation, RuleSetKey, RuleSetStore,
    RuleSetStoreError, RuleSetVersion, RuleSetVersionId, RuleSetVersionIdentity,
};
use automation_ruleset_readiness::{
    activate_if_ready, ActivationError, ReadinessError, RoleHierarchyReadinessErrorV1,
};
use chrono::Duration;
use discord_model::{GuildId, UserId};

use crate::id::{ActivationDigest, ActivationRequestId, ApplyAttemptId};
use crate::model::{
    ActivationRequest, ActivationRequestState, ActivationTarget, ActivationTerminationV1,
    ApplyErrorRecord, ApplyFailureKind, CompletionKind, CreateActivationRequest, ObservedActive,
    SupersessionReasonV1,
};
use crate::product_preflight::ProductPreflightCauseV1;
use crate::store::{
    ActivationRequestStore, ActivationStoreError, ApproveError, ClaimOutcome, RejectError,
    WithdrawError,
};
use crate::{
    assess_product_target_v1, check_product_readiness_v1, product_active_baseline_v1,
    validate_product_target_v1, ActivationApprovalContextV1, ActivationEnvironment,
    ActivationLinkStateV1, ProductApprovalContextV1, ProductPreflightErrorV1,
    ProductReadinessAssessmentV1, ProductTargetAssessmentV1, ValidatedProductTargetV1,
};

const APPLY_LEASE_SECONDS: i64 = 60;
const APPLY_DEADLINE: StdDuration = StdDuration::from_secs(45);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeadlineElapsed;

struct DeadlineState {
    is_completed: bool,
    is_timed_out: bool,
    waker: Option<Waker>,
}

struct DeadlineSignal {
    state: Mutex<DeadlineState>,
    changed: Condvar,
}

struct Deadline<F> {
    future: Pin<Box<F>>,
    signal: Arc<DeadlineSignal>,
}

impl<F: Future> Future for Deadline<F> {
    type Output = Result<F::Output, DeadlineElapsed>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.signal.state.lock().unwrap().is_timed_out {
            return Poll::Ready(Err(DeadlineElapsed));
        }
        match this.future.as_mut().poll(context) {
            Poll::Ready(output) => {
                let mut state = this.signal.state.lock().unwrap();
                if state.is_timed_out {
                    return Poll::Ready(Err(DeadlineElapsed));
                }
                state.is_completed = true;
                this.signal.changed.notify_one();
                Poll::Ready(Ok(output))
            }
            Poll::Pending => {
                let mut state = this.signal.state.lock().unwrap();
                if state.is_timed_out {
                    Poll::Ready(Err(DeadlineElapsed))
                } else {
                    state.waker = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        }
    }
}

impl<F> Drop for Deadline<F> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.signal.state.lock() {
            state.is_completed = true;
            self.signal.changed.notify_one();
        }
    }
}

fn with_deadline<F: Future>(future: F, duration: StdDuration) -> Deadline<F> {
    let signal = Arc::new(DeadlineSignal {
        state: Mutex::new(DeadlineState {
            is_completed: false,
            is_timed_out: false,
            waker: None,
        }),
        changed: Condvar::new(),
    });
    let timer_signal = signal.clone();
    thread::spawn(move || {
        let state = timer_signal.state.lock().unwrap();
        let (mut state, _) = timer_signal
            .changed
            .wait_timeout_while(state, duration, |state| !state.is_completed)
            .unwrap();
        let waker = if state.is_completed {
            None
        } else {
            state.is_timed_out = true;
            state.waker.take()
        };
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    Deadline {
        future: Box::pin(future),
        signal,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActivationEnvironmentError {
    #[error("activation environment load failed: {0}")]
    Load(String),
}

#[allow(async_fn_in_trait)]
pub trait ActivationEnvironmentProvider {
    async fn load_fresh(
        &self,
        target: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestActivation {
    pub id: ActivationRequestId,
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub requester: UserId,
    pub required_approvals: u32,
    pub ttl: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestActivationError {
    #[error("target ruleset version is missing")]
    TargetMissing,
    #[error("target ruleset artifact is corrupt")]
    TargetCorrupt,
    #[error("ruleset store error: {0:?}")]
    RuleSet(RuleSetStoreError),
    #[error(transparent)]
    Store(#[from] ActivationStoreError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Activated,
    AlreadyActive,
    RecoveredAlreadyActive,
    AlreadyApplied,
    Superseded {
        reason: SupersessionReasonV1,
    },
    InProgress {
        blocking_request_id: ActivationRequestId,
        lease_until: chrono::DateTime<chrono::Utc>,
        lease_expired: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    #[error("activation request is not approved")]
    NotApproved,
    #[error("product activation request is not linked")]
    Unlinked,
    #[error("activation request is expired")]
    Expired,
    #[error("activation request was withdrawn")]
    Withdrawn,
    #[error("activation lease is lost")]
    LeaseLost,
    #[error("target ruleset version is missing")]
    TargetMissing,
    #[error("target ruleset artifact is corrupt")]
    TargetCorrupt,
    #[error(transparent)]
    Environment(#[from] ActivationEnvironmentError),
    #[error("ruleset is not ready: {0:?}")]
    NotReady(Box<ReadinessError>),
    #[error("ruleset role hierarchy is not ready: {0}")]
    RoleHierarchyNotReady(RoleHierarchyReadinessErrorV1),
    #[error("ruleset store error: {0:?}")]
    RuleSet(RuleSetStoreError),
    #[error("activation outcome is indeterminate: {0:?}")]
    IndeterminateActivation(RuleSetStoreError),
    #[error(transparent)]
    Store(#[from] ActivationStoreError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryDisposition {
    Recovered,
    KeptApplying,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub request_id: ActivationRequestId,
    pub disposition: RecoveryDisposition,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub entries: Vec<RecoveryEntry>,
}

pub struct ActivationService<'a, A, R, P> {
    requests: &'a A,
    rulesets: &'a R,
    provider: &'a P,
}

impl<'a, A, R, P> ActivationService<'a, A, R, P> {
    pub fn new(requests: &'a A, rulesets: &'a R, provider: &'a P) -> Self {
        Self {
            requests,
            rulesets,
            provider,
        }
    }
}

impl<A, R, P> ActivationService<'_, A, R, P>
where
    A: ActivationRequestStore,
    R: RuleSetStore,
    P: ActivationEnvironmentProvider,
{
    pub async fn request_activation(
        &self,
        input: RequestActivation,
    ) -> Result<ActivationRequest, RequestActivationError> {
        let artifact = self
            .rulesets
            .get_version(input.guild_id, &input.ruleset_key, input.version)
            .await
            .map_err(RequestActivationError::RuleSet)?
            .ok_or(RequestActivationError::TargetMissing)?;
        if artifact.guild_id != input.guild_id
            || artifact.ruleset_key != input.ruleset_key
            || artifact.version != input.version
        {
            return Err(RequestActivationError::TargetCorrupt);
        }
        let observed_active = self
            .rulesets
            .active(input.guild_id, &input.ruleset_key)
            .await
            .map_err(RequestActivationError::RuleSet)?
            .map(|active| ObservedActive {
                version: active.version,
                content_hash: active.content_hash,
            });
        self.requests
            .create(CreateActivationRequest {
                id: input.id,
                target: ActivationTarget {
                    guild_id: input.guild_id,
                    ruleset_key: input.ruleset_key,
                    version: input.version,
                    content_hash: artifact.content_hash,
                },
                requester: input.requester,
                required_approvals: input.required_approvals,
                ttl: input.ttl,
                observed_active,
            })
            .await
            .map_err(RequestActivationError::Store)
    }

    pub async fn approve(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
    ) -> Result<ActivationRequest, ApproveError> {
        self.requests.approve(request_id, approver).await
    }

    pub async fn approve_bound(
        &self,
        request_id: &ActivationRequestId,
        approver: UserId,
        approval_payload_digest: &ActivationDigest,
    ) -> Result<ActivationRequest, ApproveError> {
        self.requests
            .approve_bound(request_id, approver, approval_payload_digest)
            .await
    }

    pub async fn reject(
        &self,
        request_id: &ActivationRequestId,
        rejected_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, RejectError> {
        self.requests.reject(request_id, rejected_by, reason).await
    }

    pub async fn withdraw(
        &self,
        request_id: &ActivationRequestId,
        withdrawn_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, WithdrawError> {
        self.requests
            .withdraw(request_id, withdrawn_by, reason)
            .await
    }

    pub async fn apply(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        applied_by: UserId,
    ) -> Result<ApplyOutcome, ApplyError> {
        let request = self.load_request(request_id).await?;
        if matches!(
            &request.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) && !matches!(&request.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return Err(ApplyError::Unlinked);
        }
        match request.state {
            ActivationRequestState::Applied => return Ok(ApplyOutcome::AlreadyApplied),
            ActivationRequestState::Expired => return Err(ApplyError::Expired),
            ActivationRequestState::Superseded => {
                return Ok(ApplyOutcome::Superseded {
                    reason: supersession_reason(&request)?,
                })
            }
            ActivationRequestState::Withdrawn => return Err(ApplyError::Withdrawn),
            ActivationRequestState::Pending | ActivationRequestState::Rejected => {
                return Err(ApplyError::NotApproved)
            }
            ActivationRequestState::Approved | ActivationRequestState::Applying => {}
        }
        let claim = self
            .requests
            .claim_apply(request_id, attempt_id.clone(), APPLY_LEASE_SECONDS)
            .await?;
        self.apply_claim(request_id, attempt_id, applied_by, claim)
            .await
    }

    pub async fn resume(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        applied_by: UserId,
    ) -> Result<ApplyOutcome, ApplyError> {
        let request = self.load_request(request_id).await?;
        if matches!(
            &request.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ) && !matches!(&request.link_state, ActivationLinkStateV1::Linked { .. })
        {
            return Err(ApplyError::Unlinked);
        }
        match request.state {
            ActivationRequestState::Applied => return Ok(ApplyOutcome::AlreadyApplied),
            ActivationRequestState::Expired => return Err(ApplyError::Expired),
            ActivationRequestState::Superseded => {
                return Ok(ApplyOutcome::Superseded {
                    reason: supersession_reason(&request)?,
                })
            }
            ActivationRequestState::Withdrawn => return Err(ApplyError::Withdrawn),
            ActivationRequestState::Applying => {}
            ActivationRequestState::Pending
            | ActivationRequestState::Approved
            | ActivationRequestState::Rejected => return Err(ApplyError::NotApproved),
        }
        let claim = self
            .requests
            .claim_resume(request_id, attempt_id.clone(), APPLY_LEASE_SECONDS)
            .await?;
        self.apply_claim(request_id, attempt_id, applied_by, claim)
            .await
    }

    pub async fn recover_applying(
        &self,
        guild_id: GuildId,
    ) -> Result<RecoveryReport, ActivationStoreError> {
        let applying = self.requests.list_applying(guild_id).await?;
        let mut entries = Vec::with_capacity(applying.len());
        for request in applying {
            let disposition = self.recover_one(&request).await;
            entries.push(RecoveryEntry {
                request_id: request.id,
                disposition: if disposition.as_ref().is_ok_and(|recovered| *recovered) {
                    RecoveryDisposition::Recovered
                } else {
                    RecoveryDisposition::KeptApplying
                },
                detail: disposition.err(),
            });
        }
        Ok(RecoveryReport { entries })
    }

    async fn load_request(
        &self,
        request_id: &ActivationRequestId,
    ) -> Result<ActivationRequest, ApplyError> {
        self.requests
            .get(request_id)
            .await?
            .ok_or(ApplyError::Store(ActivationStoreError::NotFound))
    }

    async fn apply_claim(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        applied_by: UserId,
        claim: ClaimOutcome,
    ) -> Result<ApplyOutcome, ApplyError> {
        let request = match claim {
            ClaimOutcome::Claimed(request) => *request,
            ClaimOutcome::InProgress {
                blocking_request_id,
                lease_until,
                lease_expired,
            } => {
                return Ok(ApplyOutcome::InProgress {
                    blocking_request_id,
                    lease_until,
                    lease_expired,
                })
            }
            ClaimOutcome::AlreadyApplied => return Ok(ApplyOutcome::AlreadyApplied),
            ClaimOutcome::NotApproved => return Err(ApplyError::NotApproved),
            ClaimOutcome::Unlinked => return Err(ApplyError::Unlinked),
            ClaimOutcome::Expired => return Err(ApplyError::Expired),
        };

        match with_deadline(
            self.apply_claimed(request_id, &attempt_id, applied_by, request),
            APPLY_DEADLINE,
        )
        .await
        {
            Ok(result) => result,
            Err(DeadlineElapsed) => Err(ApplyError::IndeterminateActivation(
                RuleSetStoreError::Backend("activation deadline exceeded".to_string()),
            )),
        }
    }

    async fn apply_claimed(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        applied_by: UserId,
        request: ActivationRequest,
    ) -> Result<ApplyOutcome, ApplyError> {
        if let ActivationApprovalContextV1::ProductAuthoring { context } = &request.approval_context
        {
            return self
                .apply_product_claimed(request_id, attempt_id, applied_by, &request, context)
                .await;
        }
        let prepared = match prepare_activation(self.rulesets, self.provider, &request.target).await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.release_known(request_id, attempt_id, &error).await?;
                return Err(error);
            }
        };

        match prepared {
            PreparedActivation::AlreadyActive => {
                self.complete(
                    request_id,
                    attempt_id,
                    applied_by,
                    CompletionKind::AlreadyActive,
                    None,
                )
                .await?;
                Ok(ApplyOutcome::AlreadyActive)
            }
            PreparedActivation::Ready(environment) => {
                if !self
                    .requests
                    .renew_lease(request_id, attempt_id, APPLY_LEASE_SECONDS)
                    .await?
                {
                    return Err(ApplyError::LeaseLost);
                }
                match execute_activation(self.rulesets, &request.target, &environment).await {
                    Ok(notices) => {
                        self.complete(
                            request_id,
                            attempt_id,
                            applied_by,
                            CompletionKind::Activated,
                            Some(notices),
                        )
                        .await?;
                        Ok(ApplyOutcome::Activated)
                    }
                    Err(error @ ApplyError::IndeterminateActivation(_)) => Err(error),
                    Err(error) => {
                        self.release_known(request_id, attempt_id, &error).await?;
                        Err(error)
                    }
                }
            }
        }
    }

    async fn apply_product_claimed(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        applied_by: UserId,
        request: &ActivationRequest,
        context: &ProductApprovalContextV1,
    ) -> Result<ApplyOutcome, ApplyError> {
        let (artifact, validated_target) =
            match load_exact_target(self.rulesets, &request.target).await {
                Ok(target) => target,
                Err(error) => {
                    self.release_known(request_id, attempt_id, &error).await?;
                    return Err(error);
                }
            };
        let active = match self
            .rulesets
            .active(request.target.guild_id, &request.target.ruleset_key)
            .await
        {
            Ok(active) => active,
            Err(error) => {
                let error = ApplyError::RuleSet(error);
                self.release_known(request_id, attempt_id, &error).await?;
                return Err(error);
            }
        };
        let active_identity = active.as_ref().map(RuleSetVersionIdentity::from);
        let target_ready =
            match assess_product_target_v1(validated_target, context, active_identity.as_ref()) {
                ProductTargetAssessmentV1::Ready(ready) => ready,
                ProductTargetAssessmentV1::Superseded { reason } => {
                    return self.supersede(request_id, attempt_id, reason).await;
                }
            };
        let environment = match self.provider.load_fresh(&request.target).await {
            Ok(environment) => environment,
            Err(error) => {
                let error = ApplyError::Environment(error);
                self.release_known(request_id, attempt_id, &error).await?;
                return Err(error);
            }
        };
        let ready = match check_product_readiness_v1(target_ready, &artifact, &environment) {
            Ok(ProductReadinessAssessmentV1::Ready(ready)) => ready,
            Ok(ProductReadinessAssessmentV1::Superseded { reason }) => {
                return self.supersede(request_id, attempt_id, reason).await;
            }
            Err(error) => {
                let error = product_preflight_apply_error(error);
                self.release_known(request_id, attempt_id, &error).await?;
                return Err(error);
            }
        };
        let target_is_active = ready.target_is_active();
        let expected_active = ready.expected_active().clone();
        let notices = Some(ready.into_activation_notices());
        if target_is_active {
            self.complete(
                request_id,
                attempt_id,
                applied_by,
                CompletionKind::AlreadyActive,
                notices,
            )
            .await?;
            return Ok(ApplyOutcome::AlreadyActive);
        }
        if !self
            .requests
            .renew_lease(request_id, attempt_id, APPLY_LEASE_SECONDS)
            .await?
        {
            return Err(ApplyError::LeaseLost);
        }
        let guarded = self
            .rulesets
            .activate_guarded(GuardedRuleSetActivation {
                guild_id: request.target.guild_id,
                ruleset_key: request.target.ruleset_key.clone(),
                target: RuleSetVersionIdentity {
                    version: request.target.version,
                    content_hash: request.target.content_hash,
                },
                expected_active,
            })
            .await;
        match guarded {
            Ok(GuardedActivationOutcome::Activated(_)) => {
                self.complete(
                    request_id,
                    attempt_id,
                    applied_by,
                    CompletionKind::Activated,
                    notices,
                )
                .await?;
                Ok(ApplyOutcome::Activated)
            }
            Ok(GuardedActivationOutcome::AlreadyTarget(_)) => {
                self.complete(
                    request_id,
                    attempt_id,
                    applied_by,
                    CompletionKind::AlreadyActive,
                    notices,
                )
                .await?;
                Ok(ApplyOutcome::AlreadyActive)
            }
            Ok(GuardedActivationOutcome::BaselineMismatch { observed_active }) => {
                self.supersede(
                    request_id,
                    attempt_id,
                    SupersessionReasonV1::ActiveBaselineDrift {
                        expected: context.baseline.clone(),
                        observed: product_active_baseline_v1(observed_active.as_ref()),
                    },
                )
                .await
            }
            Err(error @ RuleSetStoreError::Backend(_)) => {
                Err(ApplyError::IndeterminateActivation(error))
            }
            Err(RuleSetStoreError::VersionNotFound) => {
                let error = ApplyError::TargetMissing;
                self.release_known(request_id, attempt_id, &error).await?;
                Err(error)
            }
            Err(RuleSetStoreError::TargetHashMismatch) => {
                let error = ApplyError::TargetCorrupt;
                self.release_known(request_id, attempt_id, &error).await?;
                Err(error)
            }
            Err(error) => {
                let error = ApplyError::RuleSet(error);
                self.release_known(request_id, attempt_id, &error).await?;
                Err(error)
            }
        }
    }

    async fn supersede(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        reason: SupersessionReasonV1,
    ) -> Result<ApplyOutcome, ApplyError> {
        if self
            .requests
            .supersede_applying(request_id, attempt_id, reason.clone())
            .await?
        {
            Ok(ApplyOutcome::Superseded { reason })
        } else {
            Err(ApplyError::LeaseLost)
        }
    }

    async fn complete(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        applied_by: UserId,
        kind: CompletionKind,
        notices: Option<Vec<String>>,
    ) -> Result<(), ApplyError> {
        if self
            .requests
            .complete_applied(request_id, attempt_id, applied_by, kind, notices)
            .await?
        {
            Ok(())
        } else {
            Err(ApplyError::LeaseLost)
        }
    }

    async fn release_known(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: &ApplyAttemptId,
        error: &ApplyError,
    ) -> Result<(), ApplyError> {
        let record = apply_error_record(error);
        if self
            .requests
            .release_to_approved(request_id, attempt_id, record)
            .await?
        {
            Ok(())
        } else {
            Err(ApplyError::LeaseLost)
        }
    }

    async fn recover_one(&self, request: &ActivationRequest) -> Result<bool, String> {
        let target = self
            .rulesets
            .get_version(
                request.target.guild_id,
                &request.target.ruleset_key,
                request.target.version,
            )
            .await
            .map_err(|error| format!("target lookup failed: {error:?}"))?
            .ok_or_else(|| "target missing".to_string())?;
        if target.content_hash != request.target.content_hash {
            return Err("target hash mismatch".to_string());
        }
        let active = self
            .rulesets
            .active(request.target.guild_id, &request.target.ruleset_key)
            .await
            .map_err(|error| format!("active lookup failed: {error:?}"))?;
        let Some(active) = active else {
            return Ok(false);
        };
        if active.version != request.target.version
            || active.content_hash != request.target.content_hash
        {
            return Ok(false);
        }
        self.requests
            .bookkeep_applied(&request.id, request.requester)
            .await
            .map_err(|error| format!("bookkeeping failed: {error}"))
    }
}

fn supersession_reason(request: &ActivationRequest) -> Result<SupersessionReasonV1, ApplyError> {
    match request.termination.as_ref() {
        Some(ActivationTerminationV1::Superseded { reason, .. }) => Ok(reason.clone()),
        _ => Err(ApplyError::Store(ActivationStoreError::InvalidRequest(
            "superseded activation omitted terminal evidence".to_string(),
        ))),
    }
}

async fn load_exact_target<R>(
    rulesets: &R,
    target: &ActivationTarget,
) -> Result<(RuleSetVersion, ValidatedProductTargetV1), ApplyError>
where
    R: RuleSetStore,
{
    let artifact = rulesets
        .get_version(target.guild_id, &target.ruleset_key, target.version)
        .await
        .map_err(ApplyError::RuleSet)?
        .ok_or(ApplyError::TargetMissing)?;
    let validated =
        validate_product_target_v1(target, &artifact).map_err(product_preflight_apply_error)?;
    Ok((artifact, validated))
}

fn product_preflight_apply_error(error: ProductPreflightErrorV1) -> ApplyError {
    match error.into_cause() {
        ProductPreflightCauseV1::TargetCorrupt => ApplyError::TargetCorrupt,
        ProductPreflightCauseV1::BindingRevisionUnavailable => {
            ApplyError::Environment(ActivationEnvironmentError::Load(
                "authoritative binding revision is unavailable for product activation".to_string(),
            ))
        }
        ProductPreflightCauseV1::Readiness(error) => ApplyError::NotReady(error),
        ProductPreflightCauseV1::RoleHierarchy(error) => ApplyError::RoleHierarchyNotReady(error),
    }
}

enum PreparedActivation {
    AlreadyActive,
    Ready(ActivationEnvironment),
}

async fn prepare_activation<R, P>(
    rulesets: &R,
    provider: &P,
    target: &ActivationTarget,
) -> Result<PreparedActivation, ApplyError>
where
    R: RuleSetStore,
    P: ActivationEnvironmentProvider,
{
    let artifact = rulesets
        .get_version(target.guild_id, &target.ruleset_key, target.version)
        .await
        .map_err(ApplyError::RuleSet)?
        .ok_or(ApplyError::TargetMissing)?;
    if artifact.guild_id != target.guild_id
        || artifact.ruleset_key != target.ruleset_key
        || artifact.version != target.version
        || artifact.content_hash != target.content_hash
    {
        return Err(ApplyError::TargetCorrupt);
    }
    if let Some(active) = rulesets
        .active(target.guild_id, &target.ruleset_key)
        .await
        .map_err(ApplyError::RuleSet)?
    {
        if active.version == target.version {
            if active.content_hash == target.content_hash {
                return Ok(PreparedActivation::AlreadyActive);
            }
            return Err(ApplyError::TargetCorrupt);
        }
    }
    let environment = provider.load_fresh(target).await?;
    Ok(PreparedActivation::Ready(environment))
}

async fn execute_activation<R>(
    rulesets: &R,
    target: &ActivationTarget,
    environment: &ActivationEnvironment,
) -> Result<Vec<String>, ApplyError>
where
    R: RuleSetStore,
{
    let outcome = activate_if_ready(
        rulesets,
        target.guild_id,
        &target.ruleset_key,
        target.version,
        &environment.bindings,
        &environment.guild_capabilities,
        &environment.role_permissions,
    )
    .await;
    match outcome {
        Ok(outcome) => Ok(outcome
            .runtime_ruleset
            .notices
            .into_iter()
            .map(|notice| format!("{notice:?}"))
            .collect()),
        Err(ActivationError::VersionNotFound) => Err(ApplyError::TargetMissing),
        Err(ActivationError::VersionLookup(error)) => Err(ApplyError::RuleSet(error)),
        Err(ActivationError::NotReady(error)) => Err(ApplyError::NotReady(Box::new(error))),
        Err(ActivationError::Activate(RuleSetStoreError::VersionNotFound)) => {
            Err(ApplyError::TargetMissing)
        }
        Err(ActivationError::Activate(error)) => Err(ApplyError::IndeterminateActivation(error)),
    }
}

fn apply_error_record(error: &ApplyError) -> ApplyErrorRecord {
    let kind = match error {
        ApplyError::TargetMissing => ApplyFailureKind::TargetMissing,
        ApplyError::TargetCorrupt => ApplyFailureKind::TargetCorrupt,
        ApplyError::Environment(_) => ApplyFailureKind::Environment,
        ApplyError::NotReady(_) => ApplyFailureKind::NotReady,
        ApplyError::RoleHierarchyNotReady(_) => ApplyFailureKind::NotReady,
        _ => ApplyFailureKind::Activation,
    };
    ApplyErrorRecord {
        kind,
        message: error.to_string(),
    }
}

#[cfg(feature = "unsafe-dev-activation")]
pub async fn unsafe_dev_activate<R, P>(
    rulesets: &R,
    provider: &P,
    target: ActivationTarget,
    _applied_by: UserId,
) -> Result<ApplyOutcome, ApplyError>
where
    R: RuleSetStore,
    P: ActivationEnvironmentProvider,
{
    match prepare_activation(rulesets, provider, &target).await? {
        PreparedActivation::AlreadyActive => Ok(ApplyOutcome::AlreadyActive),
        PreparedActivation::Ready(environment) => {
            execute_activation(rulesets, &target, &environment).await?;
            Ok(ApplyOutcome::Activated)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::time::Duration;

    use futures::executor::block_on;

    use super::{with_deadline, DeadlineElapsed};

    #[test]
    fn deadline_returns_ready_output() {
        assert_eq!(
            block_on(with_deadline(async { 7 }, Duration::from_secs(1))),
            Ok(7)
        );
    }

    #[test]
    fn deadline_expires_pending_future() {
        assert_eq!(
            block_on(with_deadline(
                future::pending::<()>(),
                Duration::from_millis(5)
            )),
            Err(DeadlineElapsed)
        );
    }
}
