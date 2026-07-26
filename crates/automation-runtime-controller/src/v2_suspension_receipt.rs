#[cfg(test)]
mod tests;

use automation_runtime_convergence::{RuntimeDeployment, RuntimeDeploymentSnapshotV1};

use crate::v2_canonical_value::RuntimePersistenceU64V2;
use crate::{
    RuntimeAttemptDispositionV2, RuntimeExecutionReceiptV1, RuntimeResumeSuspendedAttemptV2,
    RuntimeSuspendAttemptDrainProgressV2, RuntimeSuspendAttemptOperationV2,
    RuntimeSuspendedAttemptObservationV2, RuntimeSuspendedAttemptV2, RuntimeUnixMicrosecondsV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendAttemptMutationOutcomeV2 {
    Inserted,
    Replayed,
    DrainProgressed,
    Resumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptReceiptErrorV2 {
    #[error("runtime suspension receipt does not match its immutable operation")]
    OperationMismatch,
    #[error("runtime inserted suspension receipt does not contain its initial mutable state")]
    InitialStateMismatch,
    #[error("runtime suspension drain-progress receipt does not match its exact transition")]
    DrainProgressMismatch,
    #[error("runtime suspension sidecar revision is not the exact successor")]
    SidecarRevisionMismatch,
    #[error("runtime suspension resume returned an invalid successor execution")]
    InvalidSuccessor,
    #[error("runtime suspension resume returned the wrong successor deployment revision")]
    SuccessorRevisionMismatch,
    #[error("runtime suspension resume returned the wrong successor convergence attempt")]
    SuccessorAttemptMismatch,
    #[error("runtime suspension resume returned the wrong controller")]
    SuccessorControllerMismatch,
    #[error("runtime suspension resume returned the wrong successor fence")]
    SuccessorFenceMismatch,
    #[error("runtime suspension resume returned a mismatched controller lease")]
    SuccessorLeaseMismatch,
    #[error("runtime suspension resume did not satisfy its durable retry gate")]
    SuccessorGateMismatch,
    #[error("runtime suspension resume changed preserved deployment truth")]
    PreservedTruthMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptReceiptV2 {
    outcome: RuntimeSuspendAttemptMutationOutcomeV2,
    snapshot: RuntimeDeploymentSnapshotV1,
    suspended: Option<RuntimeSuspendedAttemptV2>,
    successor_execution: Option<RuntimeExecutionReceiptV1>,
}

impl RuntimeSuspendAttemptReceiptV2 {
    pub fn inserted(
        operation: &RuntimeSuspendAttemptOperationV2,
        observation: RuntimeSuspendedAttemptObservationV2,
    ) -> Result<Self, RuntimeSuspendAttemptReceiptErrorV2> {
        let suspended = observation.suspended();
        validate_operation(operation, suspended)?;
        let request = operation.canonical_attempt().request();
        if suspended.local_effect() != &request.local_effect
            || suspended.drain_obligation() != &request.drain_obligation
        {
            return Err(RuntimeSuspendAttemptReceiptErrorV2::InitialStateMismatch);
        }
        Ok(Self::present(
            RuntimeSuspendAttemptMutationOutcomeV2::Inserted,
            observation,
        ))
    }

    pub fn replayed(
        proposed_operation: &RuntimeSuspendAttemptOperationV2,
        observation: RuntimeSuspendedAttemptObservationV2,
    ) -> Result<Self, RuntimeSuspendAttemptReceiptErrorV2> {
        validate_operation(proposed_operation, observation.suspended())?;
        Ok(Self::present(
            RuntimeSuspendAttemptMutationOutcomeV2::Replayed,
            observation,
        ))
    }

    pub fn drain_progressed(
        progress: &RuntimeSuspendAttemptDrainProgressV2,
        observation: RuntimeSuspendedAttemptObservationV2,
    ) -> Result<Self, RuntimeSuspendAttemptReceiptErrorV2> {
        validate_drain_progress(progress, observation.suspended())?;
        Ok(Self::present(
            RuntimeSuspendAttemptMutationOutcomeV2::DrainProgressed,
            observation,
        ))
    }

    pub fn resumed(
        resume: &RuntimeResumeSuspendedAttemptV2,
        successor_execution: RuntimeExecutionReceiptV1,
    ) -> Result<Self, RuntimeSuspendAttemptReceiptErrorV2> {
        validate_resumed_execution(resume, &successor_execution)?;
        Ok(Self {
            outcome: RuntimeSuspendAttemptMutationOutcomeV2::Resumed,
            snapshot: successor_execution.snapshot.clone(),
            suspended: None,
            successor_execution: Some(successor_execution),
        })
    }

    pub fn outcome(&self) -> RuntimeSuspendAttemptMutationOutcomeV2 {
        self.outcome
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub fn suspended(&self) -> Option<&RuntimeSuspendedAttemptV2> {
        self.suspended.as_ref()
    }

    pub fn successor_execution(&self) -> Option<&RuntimeExecutionReceiptV1> {
        self.successor_execution.as_ref()
    }

    fn present(
        outcome: RuntimeSuspendAttemptMutationOutcomeV2,
        observation: RuntimeSuspendedAttemptObservationV2,
    ) -> Self {
        Self {
            outcome,
            snapshot: observation.snapshot().clone(),
            suspended: Some(observation.suspended().clone()),
            successor_execution: None,
        }
    }
}

fn validate_operation(
    operation: &RuntimeSuspendAttemptOperationV2,
    suspended: &RuntimeSuspendedAttemptV2,
) -> Result<(), RuntimeSuspendAttemptReceiptErrorV2> {
    if suspended.operation_scope() != operation.operation_scope()
        || suspended.canonical_attempt() != operation.canonical_attempt()
    {
        Err(RuntimeSuspendAttemptReceiptErrorV2::OperationMismatch)
    } else {
        Ok(())
    }
}

fn validate_drain_progress(
    progress: &RuntimeSuspendAttemptDrainProgressV2,
    result: &RuntimeSuspendedAttemptV2,
) -> Result<(), RuntimeSuspendAttemptReceiptErrorV2> {
    let source = progress.source();
    if result.operation_scope() != source.operation_scope()
        || result.canonical_attempt() != source.canonical_attempt()
        || result.suspended_at() != source.suspended_at()
        || result.local_effect() != progress.replacement_local_effect()
        || result.drain_obligation() != progress.replacement_drain_obligation()
    {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::DrainProgressMismatch);
    }
    let expected_revision = source
        .sidecar_revision()
        .get()
        .checked_add(1)
        .and_then(std::num::NonZeroU64::new)
        .ok_or(RuntimeSuspendAttemptReceiptErrorV2::SidecarRevisionMismatch)?;
    RuntimePersistenceU64V2::from_non_zero(expected_revision)
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::SidecarRevisionMismatch)?;
    if result.sidecar_revision() != expected_revision {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SidecarRevisionMismatch);
    }
    Ok(())
}

fn validate_resumed_execution(
    resume: &RuntimeResumeSuspendedAttemptV2,
    successor: &RuntimeExecutionReceiptV1,
) -> Result<(), RuntimeSuspendAttemptReceiptErrorV2> {
    let source = resume.basis().snapshot();
    let snapshot = &successor.snapshot;
    RuntimeDeployment::restore(snapshot.clone())
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::InvalidSuccessor)?;
    RuntimeUnixMicrosecondsV2::from_datetime(successor.acquired_at)
        .and_then(|_| RuntimeUnixMicrosecondsV2::from_datetime(successor.expires_at))
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::InvalidSuccessor)?;

    let expected_revision = source
        .revision
        .next()
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::SuccessorRevisionMismatch)?;
    RuntimePersistenceU64V2::from_u64(expected_revision.get())
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::SuccessorRevisionMismatch)?;
    if snapshot.revision != expected_revision {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorRevisionMismatch);
    }

    let expected_attempt = resume
        .basis()
        .persisted_convergence_attempt()
        .get()
        .checked_add(1)
        .and_then(std::num::NonZeroU32::new)
        .ok_or(RuntimeSuspendAttemptReceiptErrorV2::SuccessorAttemptMismatch)?;
    if successor.convergence_attempt != expected_attempt {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorAttemptMismatch);
    }
    if &successor.controller_id != resume.controller_id() {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorControllerMismatch);
    }

    let expected_fence = resume
        .basis()
        .suspended()
        .source_guard()
        .fencing_token
        .next()
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::SuccessorFenceMismatch)?;
    RuntimePersistenceU64V2::from_u64(expected_fence.get())
        .map_err(|_| RuntimeSuspendAttemptReceiptErrorV2::SuccessorFenceMismatch)?;
    if successor.fencing_token != expected_fence
        || snapshot.last_fencing_token != Some(expected_fence)
    {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorFenceMismatch);
    }

    let lease = snapshot
        .controller_lease
        .as_ref()
        .ok_or(RuntimeSuspendAttemptReceiptErrorV2::SuccessorLeaseMismatch)?;
    if lease.controller_id != successor.controller_id
        || lease.fencing_token != successor.fencing_token
        || lease.acquired_at != successor.acquired_at
        || lease.expires_at != successor.expires_at
        || successor
            .expires_at
            .signed_duration_since(successor.acquired_at)
            .to_std()
            .ok()
            != Some(resume.lease_for())
    {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorLeaseMismatch);
    }
    if matches!(
        resume.basis().suspended().disposition(),
        RuntimeAttemptDispositionV2::Retryable { retry_not_before }
            if successor.acquired_at < *retry_not_before
    ) {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::SuccessorGateMismatch);
    }

    if !preserved_truth_matches(source, snapshot) {
        return Err(RuntimeSuspendAttemptReceiptErrorV2::PreservedTruthMismatch);
    }
    Ok(())
}

fn preserved_truth_matches(
    source: &RuntimeDeploymentSnapshotV1,
    successor: &RuntimeDeploymentSnapshotV1,
) -> bool {
    source.identity == successor.identity
        && source.target == successor.target
        && source.runtime_generation == successor.runtime_generation
        && source.previous_runtime == successor.previous_runtime
        && source.requested_at == successor.requested_at
        && source.phase == successor.phase
        && source.preflight == successor.preflight
        && source.drain == successor.drain
        && source.activation == successor.activation
        && source.panel_certificate == successor.panel_certificate
        && source.gateway_ready == successor.gateway_ready
        && source.live == successor.live
        && source.last_live_recovery == successor.last_live_recovery
        && source.last_runtime_failure == successor.last_runtime_failure
}
