#[cfg(test)]
mod tests;

use automation_runtime_convergence::{RuntimeDeployment, RuntimeDeploymentSnapshotV1};

use crate::v2_suspension::{
    suspension_current_target_matches, suspension_previous_runtime_matches,
    suspension_source_evidence_at,
};
use crate::{RuntimeLocalRouteEffectV2, RuntimeSuspendedAttemptV2, RuntimeSuspensionSourcePhaseV2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendedAttemptObservationFieldV2 {
    Scope,
    DeploymentRevision,
    RuntimeGeneration,
    SourcePhase,
    CurrentTarget,
    PreviousRuntime,
    FailureRecordedAt,
    ControllerId,
    FencingToken,
    LastFencingToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendedAttemptObservationErrorV2 {
    #[error("runtime suspended-attempt observation snapshot is invalid")]
    InvalidSnapshot,
    #[error("runtime suspended-attempt observation disagrees on {field:?}")]
    CorrelationMismatch {
        field: RuntimeSuspendedAttemptObservationFieldV2,
    },
    #[error("runtime suspended-attempt observation lost a process-local route lease")]
    LocalRouteLeaseMissing,
    #[error("runtime suspended-attempt observation is not quiescent")]
    NotQuiescent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendedAttemptObservationKindV2 {
    LocalRoutePresent,
    ReleasePending,
    LocallyQuiescent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendedAttemptObservationV2 {
    kind: RuntimeSuspendedAttemptObservationKindV2,
    snapshot: RuntimeDeploymentSnapshotV1,
    suspended: RuntimeSuspendedAttemptV2,
}

impl RuntimeSuspendedAttemptObservationV2 {
    pub fn new(
        snapshot: RuntimeDeploymentSnapshotV1,
        suspended: RuntimeSuspendedAttemptV2,
    ) -> Result<Self, RuntimeSuspendedAttemptObservationErrorV2> {
        validate_snapshot(&snapshot, &suspended)?;
        let lease = snapshot.controller_lease.as_ref();
        let kind = match (suspended.local_effect(), lease) {
            (RuntimeLocalRouteEffectV2::ExactRoute { .. }, Some(_)) => {
                RuntimeSuspendedAttemptObservationKindV2::LocalRoutePresent
            }
            (RuntimeLocalRouteEffectV2::ExactRoute { .. }, None) => {
                return Err(RuntimeSuspendedAttemptObservationErrorV2::LocalRouteLeaseMissing);
            }
            (
                RuntimeLocalRouteEffectV2::None | RuntimeLocalRouteEffectV2::RouteAbsent { .. },
                Some(_),
            ) => RuntimeSuspendedAttemptObservationKindV2::ReleasePending,
            (
                RuntimeLocalRouteEffectV2::None | RuntimeLocalRouteEffectV2::RouteAbsent { .. },
                None,
            ) => RuntimeSuspendedAttemptObservationKindV2::LocallyQuiescent,
        };
        Ok(Self {
            kind,
            snapshot,
            suspended,
        })
    }

    pub fn kind(&self) -> RuntimeSuspendedAttemptObservationKindV2 {
        self.kind
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub fn suspended(&self) -> &RuntimeSuspendedAttemptV2 {
        &self.suspended
    }

    pub fn into_locally_quiescent(
        self,
    ) -> Result<RuntimeLocallyQuiescentSuspendedAttemptV2, RuntimeSuspendedAttemptObservationErrorV2>
    {
        RuntimeLocallyQuiescentSuspendedAttemptV2::try_from(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLocallyQuiescentSuspendedAttemptV2 {
    observation: RuntimeSuspendedAttemptObservationV2,
}

impl RuntimeLocallyQuiescentSuspendedAttemptV2 {
    pub fn observation(&self) -> &RuntimeSuspendedAttemptObservationV2 {
        &self.observation
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        self.observation.snapshot()
    }

    pub fn suspended(&self) -> &RuntimeSuspendedAttemptV2 {
        self.observation.suspended()
    }
}

impl TryFrom<RuntimeSuspendedAttemptObservationV2> for RuntimeLocallyQuiescentSuspendedAttemptV2 {
    type Error = RuntimeSuspendedAttemptObservationErrorV2;

    fn try_from(observation: RuntimeSuspendedAttemptObservationV2) -> Result<Self, Self::Error> {
        if observation.kind() != RuntimeSuspendedAttemptObservationKindV2::LocallyQuiescent {
            return Err(RuntimeSuspendedAttemptObservationErrorV2::NotQuiescent);
        }
        Ok(Self { observation })
    }
}

fn validate_snapshot(
    snapshot: &RuntimeDeploymentSnapshotV1,
    suspended: &RuntimeSuspendedAttemptV2,
) -> Result<(), RuntimeSuspendedAttemptObservationErrorV2> {
    RuntimeDeployment::restore(snapshot.clone())
        .map_err(|_| RuntimeSuspendedAttemptObservationErrorV2::InvalidSnapshot)?;
    let operation_scope = suspended.operation_scope();
    let guard = suspended.source_guard();
    let field = if !operation_scope.scope().matches(&snapshot.identity) {
        Some(RuntimeSuspendedAttemptObservationFieldV2::Scope)
    } else if operation_scope.deployment_revision() != snapshot.revision {
        Some(RuntimeSuspendedAttemptObservationFieldV2::DeploymentRevision)
    } else if guard.runtime_generation != snapshot.runtime_generation {
        Some(RuntimeSuspendedAttemptObservationFieldV2::RuntimeGeneration)
    } else if RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&snapshot.phase)
        != Some(suspended.source_phase())
    {
        Some(RuntimeSuspendedAttemptObservationFieldV2::SourcePhase)
    } else if !suspension_current_target_matches(
        suspended.local_effect(),
        suspended.drain_obligation(),
        &snapshot.target,
    ) {
        Some(RuntimeSuspendedAttemptObservationFieldV2::CurrentTarget)
    } else if !suspension_previous_runtime_matches(
        suspended.drain_obligation(),
        snapshot.previous_runtime.as_ref(),
        &snapshot.target,
    ) {
        Some(RuntimeSuspendedAttemptObservationFieldV2::PreviousRuntime)
    } else if suspended.failure().recorded_at
        < suspension_source_evidence_at(snapshot, suspended.source_phase())
            .ok_or(RuntimeSuspendedAttemptObservationErrorV2::InvalidSnapshot)?
    {
        Some(RuntimeSuspendedAttemptObservationFieldV2::FailureRecordedAt)
    } else if snapshot
        .controller_lease
        .as_ref()
        .is_some_and(|lease| lease.controller_id != suspended.source_guard().controller_id)
    {
        Some(RuntimeSuspendedAttemptObservationFieldV2::ControllerId)
    } else if snapshot
        .controller_lease
        .as_ref()
        .is_some_and(|lease| lease.fencing_token != suspended.source_guard().fencing_token)
    {
        Some(RuntimeSuspendedAttemptObservationFieldV2::FencingToken)
    } else if snapshot.last_fencing_token != Some(guard.fencing_token) {
        Some(RuntimeSuspendedAttemptObservationFieldV2::LastFencingToken)
    } else {
        None
    };
    if let Some(field) = field {
        Err(mismatch(field))
    } else {
        Ok(())
    }
}

fn mismatch(
    field: RuntimeSuspendedAttemptObservationFieldV2,
) -> RuntimeSuspendedAttemptObservationErrorV2 {
    RuntimeSuspendedAttemptObservationErrorV2::CorrelationMismatch { field }
}
