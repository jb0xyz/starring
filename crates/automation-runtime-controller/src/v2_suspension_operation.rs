#[cfg(test)]
mod tests;

use std::num::NonZeroU32;

use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, Utc};

use crate::v2_suspension::{
    suspension_current_target_matches, suspension_previous_runtime_matches,
    suspension_source_evidence_at,
};
use crate::{
    RuntimeCanonicalSuspendAttemptV2, RuntimeDeploymentScopeV1, RuntimeExecutionReceiptV1,
    RuntimeSuspendAttemptCanonicalErrorV2, RuntimeSuspendAttemptDigestV2, RuntimeSuspensionIdV2,
    RuntimeSuspensionSourcePhaseV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptOperationScopeV2 {
    scope: RuntimeDeploymentScopeV1,
    deployment_revision: DeploymentRevision,
    convergence_attempt: NonZeroU32,
}

impl RuntimeSuspendAttemptOperationScopeV2 {
    pub fn from_suspendable_execution(
        execution: &RuntimeExecutionReceiptV1,
    ) -> Result<Self, RuntimeSuspendAttemptOperationBuildErrorV2> {
        validate_execution_receipt(execution)?;
        RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&execution.snapshot.phase)
            .ok_or(RuntimeSuspendAttemptOperationBuildErrorV2::SourcePhaseNotSuspendable)?;
        Ok(Self {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            deployment_revision: execution.snapshot.revision,
            convergence_attempt: execution.convergence_attempt,
        })
    }

    pub fn scope(&self) -> &RuntimeDeploymentScopeV1 {
        &self.scope
    }

    pub fn deployment_revision(&self) -> DeploymentRevision {
        self.deployment_revision
    }

    pub fn convergence_attempt(&self) -> NonZeroU32 {
        self.convergence_attempt
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendAttemptOperationFieldV2 {
    Scope,
    DeploymentRevision,
    ConvergenceAttempt,
    ControllerId,
    FencingToken,
    RuntimeGeneration,
    SourcePhase,
    CurrentTarget,
    PreviousRuntime,
    FailureRecordedAt,
    SuspensionId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptOperationBuildErrorV2 {
    #[error("runtime suspend-attempt execution receipt is invalid")]
    InvalidExecutionReceipt,
    #[error("runtime suspend-attempt source phase is not suspendable")]
    SourcePhaseNotSuspendable,
    #[error("runtime suspend-attempt request disagrees with its execution on {field:?}")]
    RequestCorrelationMismatch {
        field: RuntimeSuspendAttemptOperationFieldV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptOperationPersistenceErrorV2 {
    #[error(transparent)]
    Canonical(#[from] RuntimeSuspendAttemptCanonicalErrorV2),
    #[error("persisted suspend-attempt operation disagrees on {field:?}")]
    PersistedCorrelationMismatch {
        field: RuntimeSuspendAttemptOperationFieldV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptReplayErrorV2 {
    #[error("persisted suspend-attempt root does not match the proposed creation")]
    CreationMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptOperationV2 {
    operation_scope: RuntimeSuspendAttemptOperationScopeV2,
    source_target: RuntimeDeploymentTargetV1,
    source_previous_runtime: Option<RuntimeProcessIdentityV1>,
    source_evidence_at: DateTime<Utc>,
    canonical_attempt: RuntimeCanonicalSuspendAttemptV2,
}

impl RuntimeSuspendAttemptOperationV2 {
    pub fn new(
        execution: &RuntimeExecutionReceiptV1,
        canonical_attempt: RuntimeCanonicalSuspendAttemptV2,
    ) -> Result<Self, RuntimeSuspendAttemptOperationBuildErrorV2> {
        let operation_scope =
            RuntimeSuspendAttemptOperationScopeV2::from_suspendable_execution(execution)?;
        let source_evidence_at = validate_attempt_against_execution(execution, &canonical_attempt)?;
        Ok(Self {
            operation_scope,
            source_target: execution.snapshot.target.clone(),
            source_previous_runtime: execution.snapshot.previous_runtime.clone(),
            source_evidence_at,
            canonical_attempt,
        })
    }

    pub fn operation_scope(&self) -> &RuntimeSuspendAttemptOperationScopeV2 {
        &self.operation_scope
    }

    pub fn source_target(&self) -> &RuntimeDeploymentTargetV1 {
        &self.source_target
    }

    pub fn source_previous_runtime(&self) -> Option<&RuntimeProcessIdentityV1> {
        self.source_previous_runtime.as_ref()
    }

    pub fn source_evidence_at(&self) -> DateTime<Utc> {
        self.source_evidence_at
    }

    pub fn canonical_attempt(&self) -> &RuntimeCanonicalSuspendAttemptV2 {
        &self.canonical_attempt
    }

    pub fn suspension_id(&self) -> &RuntimeSuspensionIdV2 {
        &self.canonical_attempt.request().suspension_id
    }

    pub fn suspend_attempt_request_bytes(&self) -> &[u8] {
        self.canonical_attempt.suspend_attempt_request_bytes()
    }

    pub fn suspend_attempt_digest(&self) -> &RuntimeSuspendAttemptDigestV2 {
        self.canonical_attempt.suspend_attempt_digest()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePersistedSuspendAttemptRootV2 {
    operation_scope: RuntimeSuspendAttemptOperationScopeV2,
    canonical_attempt: RuntimeCanonicalSuspendAttemptV2,
}

impl RuntimePersistedSuspendAttemptRootV2 {
    pub fn from_persisted(
        persisted_scope: RuntimeDeploymentScopeV1,
        persisted_deployment_revision: DeploymentRevision,
        persisted_convergence_attempt: NonZeroU32,
        persisted_suspension_id: &RuntimeSuspensionIdV2,
        suspend_attempt_request_bytes: &[u8],
        persisted_digest: &RuntimeSuspendAttemptDigestV2,
    ) -> Result<Self, RuntimeSuspendAttemptOperationPersistenceErrorV2> {
        let canonical_attempt = RuntimeCanonicalSuspendAttemptV2::from_persisted(
            suspend_attempt_request_bytes,
            persisted_digest,
        )?;
        let request = canonical_attempt.request();
        if persisted_scope != request.guard.scope {
            return Err(persistence_mismatch(
                RuntimeSuspendAttemptOperationFieldV2::Scope,
            ));
        }
        if persisted_deployment_revision != request.guard.expected_revision {
            return Err(persistence_mismatch(
                RuntimeSuspendAttemptOperationFieldV2::DeploymentRevision,
            ));
        }
        if persisted_convergence_attempt != request.guard.convergence_attempt {
            return Err(persistence_mismatch(
                RuntimeSuspendAttemptOperationFieldV2::ConvergenceAttempt,
            ));
        }
        if persisted_suspension_id != &request.suspension_id {
            return Err(persistence_mismatch(
                RuntimeSuspendAttemptOperationFieldV2::SuspensionId,
            ));
        }
        Ok(Self {
            operation_scope: RuntimeSuspendAttemptOperationScopeV2 {
                scope: persisted_scope,
                deployment_revision: persisted_deployment_revision,
                convergence_attempt: persisted_convergence_attempt,
            },
            canonical_attempt,
        })
    }

    pub fn operation_scope(&self) -> &RuntimeSuspendAttemptOperationScopeV2 {
        &self.operation_scope
    }

    pub fn suspension_id(&self) -> &RuntimeSuspensionIdV2 {
        &self.canonical_attempt.request().suspension_id
    }

    pub fn canonical_attempt(&self) -> &RuntimeCanonicalSuspendAttemptV2 {
        &self.canonical_attempt
    }

    pub fn suspend_attempt_request_bytes(&self) -> &[u8] {
        self.canonical_attempt.suspend_attempt_request_bytes()
    }

    pub fn suspend_attempt_digest(&self) -> &RuntimeSuspendAttemptDigestV2 {
        self.canonical_attempt.suspend_attempt_digest()
    }

    pub fn require_byte_exact_replay(
        &self,
        proposed: &RuntimeSuspendAttemptOperationV2,
    ) -> Result<(), RuntimeSuspendAttemptReplayErrorV2> {
        if self.operation_scope == *proposed.operation_scope()
            && self.suspension_id() == proposed.suspension_id()
            && self.suspend_attempt_request_bytes() == proposed.suspend_attempt_request_bytes()
            && self.suspend_attempt_digest() == proposed.suspend_attempt_digest()
        {
            Ok(())
        } else {
            Err(RuntimeSuspendAttemptReplayErrorV2::CreationMismatch)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptScopeLookupV2 {
    operation_scope: RuntimeSuspendAttemptOperationScopeV2,
}

impl RuntimeSuspendAttemptScopeLookupV2 {
    pub fn from_suspendable_execution(
        execution: &RuntimeExecutionReceiptV1,
    ) -> Result<Self, RuntimeSuspendAttemptOperationBuildErrorV2> {
        Ok(Self {
            operation_scope: RuntimeSuspendAttemptOperationScopeV2::from_suspendable_execution(
                execution,
            )?,
        })
    }

    pub fn operation_scope(&self) -> &RuntimeSuspendAttemptOperationScopeV2 {
        &self.operation_scope
    }
}

fn validate_execution_receipt(
    execution: &RuntimeExecutionReceiptV1,
) -> Result<(), RuntimeSuspendAttemptOperationBuildErrorV2> {
    RuntimeDeployment::restore(execution.snapshot.clone())
        .map_err(|_| RuntimeSuspendAttemptOperationBuildErrorV2::InvalidExecutionReceipt)?;
    let lease = execution
        .snapshot
        .controller_lease
        .as_ref()
        .ok_or(RuntimeSuspendAttemptOperationBuildErrorV2::InvalidExecutionReceipt)?;
    if lease.controller_id != execution.controller_id
        || lease.fencing_token != execution.fencing_token
        || lease.acquired_at != execution.acquired_at
        || lease.expires_at != execution.expires_at
        || execution.snapshot.last_fencing_token != Some(execution.fencing_token)
        || execution.expires_at <= execution.acquired_at
    {
        return Err(RuntimeSuspendAttemptOperationBuildErrorV2::InvalidExecutionReceipt);
    }
    Ok(())
}

fn validate_attempt_against_execution(
    execution: &RuntimeExecutionReceiptV1,
    canonical_attempt: &RuntimeCanonicalSuspendAttemptV2,
) -> Result<DateTime<Utc>, RuntimeSuspendAttemptOperationBuildErrorV2> {
    let request = canonical_attempt.request();
    let guard = &request.guard;
    let expected_scope = RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity);
    let expected_phase =
        RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&execution.snapshot.phase)
            .ok_or(RuntimeSuspendAttemptOperationBuildErrorV2::SourcePhaseNotSuspendable)?;
    let mismatch = if guard.scope != expected_scope {
        Some(RuntimeSuspendAttemptOperationFieldV2::Scope)
    } else if guard.expected_revision != execution.snapshot.revision {
        Some(RuntimeSuspendAttemptOperationFieldV2::DeploymentRevision)
    } else if guard.convergence_attempt != execution.convergence_attempt {
        Some(RuntimeSuspendAttemptOperationFieldV2::ConvergenceAttempt)
    } else if guard.controller_id != execution.controller_id {
        Some(RuntimeSuspendAttemptOperationFieldV2::ControllerId)
    } else if guard.fencing_token != execution.fencing_token {
        Some(RuntimeSuspendAttemptOperationFieldV2::FencingToken)
    } else if guard.runtime_generation != execution.snapshot.runtime_generation {
        Some(RuntimeSuspendAttemptOperationFieldV2::RuntimeGeneration)
    } else if request.source_phase != expected_phase {
        Some(RuntimeSuspendAttemptOperationFieldV2::SourcePhase)
    } else if !suspension_current_target_matches(
        &request.local_effect,
        &request.drain_obligation,
        &execution.snapshot.target,
    ) {
        Some(RuntimeSuspendAttemptOperationFieldV2::CurrentTarget)
    } else if !suspension_previous_runtime_matches(
        &request.drain_obligation,
        execution.snapshot.previous_runtime.as_ref(),
        &execution.snapshot.target,
    ) {
        Some(RuntimeSuspendAttemptOperationFieldV2::PreviousRuntime)
    } else {
        None
    };
    if let Some(field) = mismatch {
        return Err(
            RuntimeSuspendAttemptOperationBuildErrorV2::RequestCorrelationMismatch { field },
        );
    }
    let evidence_at = suspension_source_evidence_at(&execution.snapshot, expected_phase)
        .ok_or(RuntimeSuspendAttemptOperationBuildErrorV2::InvalidExecutionReceipt)?;
    if request.failure.recorded_at < evidence_at {
        return Err(
            RuntimeSuspendAttemptOperationBuildErrorV2::RequestCorrelationMismatch {
                field: RuntimeSuspendAttemptOperationFieldV2::FailureRecordedAt,
            },
        );
    }
    Ok(evidence_at)
}

fn persistence_mismatch(
    field: RuntimeSuspendAttemptOperationFieldV2,
) -> RuntimeSuspendAttemptOperationPersistenceErrorV2 {
    RuntimeSuspendAttemptOperationPersistenceErrorV2::PersistedCorrelationMismatch { field }
}
