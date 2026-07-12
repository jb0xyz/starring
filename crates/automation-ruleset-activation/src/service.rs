use std::collections::BTreeMap;

use automation_ruleset::{RuleSetKey, RuleSetStore, RuleSetStoreError, RuleSetVersionId};
use automation_ruleset_readiness::{
    activate_if_ready, ActivationError, GuildCapabilities, ReadinessError,
};
use chrono::Duration;
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, UserId};
use resource_resolution::ResourceBindingMap;

use crate::id::{ActivationRequestId, ApplyAttemptId};
use crate::model::{
    ActivationRequest, ActivationRequestState, ActivationTarget, ApplyErrorRecord,
    ApplyFailureKind, CompletionKind, CreateActivationRequest, ObservedActive,
};
use crate::store::{
    ActivationRequestStore, ActivationStoreError, ApproveError, ClaimOutcome, RejectError,
};

const APPLY_LEASE_SECONDS: i64 = 60;

pub struct ActivationEnvironment {
    pub bindings: ResourceBindingMap,
    pub guild_capabilities: GuildCapabilities,
    pub role_permissions: BTreeMap<ResourceKey, Permissions>,
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
    #[error("activation request is expired")]
    Expired,
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

    pub async fn reject(
        &self,
        request_id: &ActivationRequestId,
        rejected_by: UserId,
        reason: String,
    ) -> Result<ActivationRequest, RejectError> {
        self.requests.reject(request_id, rejected_by, reason).await
    }

    pub async fn apply(
        &self,
        request_id: &ActivationRequestId,
        attempt_id: ApplyAttemptId,
        applied_by: UserId,
    ) -> Result<ApplyOutcome, ApplyError> {
        let request = self.load_request(request_id).await?;
        match request.state {
            ActivationRequestState::Applied => return Ok(ApplyOutcome::AlreadyApplied),
            ActivationRequestState::Expired => return Err(ApplyError::Expired),
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
        match request.state {
            ActivationRequestState::Applied => return Ok(ApplyOutcome::AlreadyApplied),
            ActivationRequestState::Expired => return Err(ApplyError::Expired),
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
            ClaimOutcome::Expired => return Err(ApplyError::Expired),
        };

        let prepared = match prepare_activation(self.rulesets, self.provider, &request.target).await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.release_known(request_id, &attempt_id, &error).await?;
                return Err(error);
            }
        };

        match prepared {
            PreparedActivation::AlreadyActive => {
                self.complete(
                    request_id,
                    &attempt_id,
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
                    .renew_lease(request_id, &attempt_id, APPLY_LEASE_SECONDS)
                    .await?
                {
                    return Err(ApplyError::LeaseLost);
                }
                match execute_activation(self.rulesets, &request.target, &environment).await {
                    Ok(notices) => {
                        self.complete(
                            request_id,
                            &attempt_id,
                            applied_by,
                            CompletionKind::Activated,
                            Some(notices),
                        )
                        .await?;
                        Ok(ApplyOutcome::Activated)
                    }
                    Err(error @ ApplyError::IndeterminateActivation(_)) => Err(error),
                    Err(error) => {
                        self.release_known(request_id, &attempt_id, &error).await?;
                        Err(error)
                    }
                }
            }
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
