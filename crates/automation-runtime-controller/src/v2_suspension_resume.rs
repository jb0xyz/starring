#[cfg(test)]
mod tests;

use std::num::NonZeroU32;
use std::time::Duration;

use automation_runtime_convergence::{ControllerId, RuntimeDeploymentSnapshotV1, RuntimeFailureId};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeAttemptDispositionV2, RuntimeLocallyQuiescentSuspendedAttemptV2,
    RuntimeSuspendedAttemptV2, RuntimeUnixMicrosecondsV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendAttemptResumeGateV2 {
    Retryable {
        database_observed_at: DateTime<Utc>,
    },
    RecoverBlocked {
        expected_failure_id: RuntimeFailureId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptResumeBasisErrorV2 {
    #[error("runtime suspension resume observation has an invalid database timestamp")]
    InvalidDatabaseObservation,
    #[error("runtime suspension resume observation has the wrong convergence attempt")]
    ConvergenceAttemptMismatch,
    #[error("runtime suspension resume observation has the wrong last controller")]
    LastControllerMismatch,
    #[error("runtime suspension resume gate does not match its disposition")]
    DispositionMismatch,
    #[error("runtime suspension retry is not ready")]
    RetryNotReady,
    #[error("runtime blocked suspension recovery has the wrong failure ID")]
    FailureIdMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptResumeBasisV2 {
    locally_quiescent: RuntimeLocallyQuiescentSuspendedAttemptV2,
    persisted_convergence_attempt: NonZeroU32,
    persisted_last_controller_id: ControllerId,
    gate: RuntimeSuspendAttemptResumeGateV2,
}

impl RuntimeSuspendAttemptResumeBasisV2 {
    pub fn new(
        locally_quiescent: RuntimeLocallyQuiescentSuspendedAttemptV2,
        persisted_convergence_attempt: NonZeroU32,
        persisted_last_controller_id: ControllerId,
        gate: RuntimeSuspendAttemptResumeGateV2,
    ) -> Result<Self, RuntimeSuspendAttemptResumeBasisErrorV2> {
        let suspended = locally_quiescent.suspended();
        if persisted_convergence_attempt != suspended.operation_scope().convergence_attempt() {
            return Err(RuntimeSuspendAttemptResumeBasisErrorV2::ConvergenceAttemptMismatch);
        }
        if persisted_last_controller_id != suspended.source_guard().controller_id {
            return Err(RuntimeSuspendAttemptResumeBasisErrorV2::LastControllerMismatch);
        }
        validate_resume_gate(suspended, &gate)?;
        Ok(Self {
            locally_quiescent,
            persisted_convergence_attempt,
            persisted_last_controller_id,
            gate,
        })
    }

    pub fn locally_quiescent(&self) -> &RuntimeLocallyQuiescentSuspendedAttemptV2 {
        &self.locally_quiescent
    }

    pub fn suspended(&self) -> &RuntimeSuspendedAttemptV2 {
        self.locally_quiescent.suspended()
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        self.locally_quiescent.snapshot()
    }

    pub fn persisted_convergence_attempt(&self) -> NonZeroU32 {
        self.persisted_convergence_attempt
    }

    pub fn persisted_last_controller_id(&self) -> &ControllerId {
        &self.persisted_last_controller_id
    }

    pub fn gate(&self) -> &RuntimeSuspendAttemptResumeGateV2 {
        &self.gate
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeResumeSuspendedAttemptErrorV2 {
    #[error("runtime suspension resume lease duration is invalid")]
    InvalidLeaseDuration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeControllerLeaseDurationV2(Duration);

impl RuntimeControllerLeaseDurationV2 {
    pub fn new(value: Duration) -> Result<Self, RuntimeResumeSuspendedAttemptErrorV2> {
        let milliseconds = value.as_millis();
        if !value.subsec_nanos().is_multiple_of(1_000_000)
            || !(1_000..=600_000).contains(&milliseconds)
        {
            return Err(RuntimeResumeSuspendedAttemptErrorV2::InvalidLeaseDuration);
        }
        Ok(Self(value))
    }

    pub fn get(self) -> Duration {
        self.0
    }

    pub fn milliseconds(self) -> u64 {
        self.0.as_millis() as u64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeResumeSuspendedAttemptV2 {
    basis: RuntimeSuspendAttemptResumeBasisV2,
    controller_id: ControllerId,
    lease_for: RuntimeControllerLeaseDurationV2,
}

impl RuntimeResumeSuspendedAttemptV2 {
    pub fn new(
        basis: RuntimeSuspendAttemptResumeBasisV2,
        controller_id: ControllerId,
        lease_for: Duration,
    ) -> Result<Self, RuntimeResumeSuspendedAttemptErrorV2> {
        let lease_for = RuntimeControllerLeaseDurationV2::new(lease_for)?;
        Ok(Self {
            basis,
            controller_id,
            lease_for,
        })
    }

    pub fn basis(&self) -> &RuntimeSuspendAttemptResumeBasisV2 {
        &self.basis
    }

    pub fn controller_id(&self) -> &ControllerId {
        &self.controller_id
    }

    pub fn lease_for(&self) -> Duration {
        self.lease_for.get()
    }

    pub fn lease_duration(&self) -> RuntimeControllerLeaseDurationV2 {
        self.lease_for
    }
}

fn validate_resume_gate(
    suspended: &RuntimeSuspendedAttemptV2,
    gate: &RuntimeSuspendAttemptResumeGateV2,
) -> Result<(), RuntimeSuspendAttemptResumeBasisErrorV2> {
    match (suspended.disposition(), gate) {
        (
            RuntimeAttemptDispositionV2::Retryable { retry_not_before },
            RuntimeSuspendAttemptResumeGateV2::Retryable {
                database_observed_at,
            },
        ) => {
            if RuntimeUnixMicrosecondsV2::from_datetime(*database_observed_at).is_err() {
                return Err(RuntimeSuspendAttemptResumeBasisErrorV2::InvalidDatabaseObservation);
            }
            if database_observed_at < retry_not_before {
                return Err(RuntimeSuspendAttemptResumeBasisErrorV2::RetryNotReady);
            }
            Ok(())
        }
        (
            RuntimeAttemptDispositionV2::Blocked,
            RuntimeSuspendAttemptResumeGateV2::RecoverBlocked {
                expected_failure_id,
            },
        ) if expected_failure_id == &suspended.failure().failure_id => Ok(()),
        (
            RuntimeAttemptDispositionV2::Blocked,
            RuntimeSuspendAttemptResumeGateV2::RecoverBlocked { .. },
        ) => Err(RuntimeSuspendAttemptResumeBasisErrorV2::FailureIdMismatch),
        (
            RuntimeAttemptDispositionV2::Retryable { .. },
            RuntimeSuspendAttemptResumeGateV2::RecoverBlocked { .. },
        )
        | (
            RuntimeAttemptDispositionV2::Blocked,
            RuntimeSuspendAttemptResumeGateV2::Retryable { .. },
        ) => Err(RuntimeSuspendAttemptResumeBasisErrorV2::DispositionMismatch),
    }
}
