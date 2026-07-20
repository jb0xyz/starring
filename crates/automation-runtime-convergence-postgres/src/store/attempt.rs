use automation_runtime_convergence::{
    DeploymentRevision, FencingToken, RuntimeDeploymentPhaseV1, RuntimeFailureDispositionV1,
    RuntimeFailureId, RuntimeGeneration, RuntimePendingConditionV1,
};
use chrono::{DateTime, Utc};

use crate::model::DeploymentMutationV1;
use crate::row::PersistedDeployment;
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

pub(super) fn next_claim_attempt(
    persisted: &PersistedDeployment,
    now: DateTime<Utc>,
) -> Result<std::num::NonZeroU32, RuntimeConvergenceStoreError> {
    let current = persisted.exact_convergence_attempt()?;
    let starts_new = persisted
        .deployment
        .controller_lease()
        .is_none_or(|lease| lease.expires_at <= now);
    let next = if starts_new {
        current
            .checked_next()
            .ok_or(RuntimeConvergenceStoreError::ConvergenceAttemptOverflow)?
    } else {
        current
            .started()
            .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                "active runtime convergence attempt",
            ))?
    };
    match persisted.deployment.phase() {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    retry_not_before, ..
                },
        } if *retry_not_before > now => Err(RuntimeConvergenceStoreError::RetryNotReady),
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. },
        } => Err(RuntimeConvergenceStoreError::OperatorActionRequired),
        _ => Ok(next),
    }
}

pub(super) fn next_blocked_recovery_attempt(
    persisted: &PersistedDeployment,
    failure_id: &RuntimeFailureId,
    failure_attempt: std::num::NonZeroU32,
) -> Result<std::num::NonZeroU32, RuntimeConvergenceStoreError> {
    match persisted.deployment.phase() {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { failure },
        } if failure.failure_id == *failure_id
            && persisted.last_failure_attempt == Some(failure_attempt)
            && persisted.deployment.controller_lease().is_none()
            && persisted.exact_convergence_attempt()?.started() == Some(failure_attempt) =>
        {
            persisted
                .exact_convergence_attempt()?
                .checked_next()
                .ok_or(RuntimeConvergenceStoreError::ConvergenceAttemptOverflow)
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. },
        } => Err(RuntimeConvergenceStoreError::ConvergenceAttemptConflict),
        _ => Err(RuntimeConvergenceStoreError::OperatorActionRequired),
    }
}

pub(super) fn validate_blocked_recovery_replay(
    persisted: &PersistedDeployment,
    failure_id: &RuntimeFailureId,
    failure_attempt: std::num::NonZeroU32,
) -> Result<(), RuntimeConvergenceStoreError> {
    let snapshot = persisted.deployment.snapshot();
    let exact_failure = matches!(
        snapshot.last_runtime_failure,
        Some(RuntimeFailureDispositionV1::Blocked { failure })
            if failure.failure_id == *failure_id
    );
    if exact_failure
        && persisted.last_failure_attempt == Some(failure_attempt)
        && persisted
            .exact_convergence_attempt()?
            .started()
            .is_some_and(|attempt| attempt > failure_attempt)
        && matches!(
            snapshot.phase,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready
            }
        )
    {
        Ok(())
    } else {
        Err(RuntimeConvergenceStoreError::ConvergenceAttemptConflict)
    }
}

pub(super) fn validate_runtime_resume(
    persisted: &PersistedDeployment,
    now: DateTime<Utc>,
) -> Result<(), RuntimeConvergenceStoreError> {
    match persisted.deployment.phase() {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    attempt,
                    retry_not_before,
                    ..
                },
        } => {
            let current = persisted
                .exact_convergence_attempt()?
                .started()
                .ok_or(RuntimeConvergenceStoreError::ConvergenceAttemptConflict)?;
            if persisted.last_failure_attempt != Some(*attempt) || current <= *attempt {
                return Err(RuntimeConvergenceStoreError::ConvergenceAttemptConflict);
            }
            if now < *retry_not_before {
                return Err(RuntimeConvergenceStoreError::RetryNotReady);
            }
            Ok(())
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. },
        } => Err(RuntimeConvergenceStoreError::OperatorActionRequired),
        _ => Ok(()),
    }
}

pub(super) fn failure_replay(
    adapter: &PostgresRuntimeConvergence,
    persisted: &PersistedDeployment,
    expected_revision: DeploymentRevision,
    fencing_token: FencingToken,
    runtime_generation: RuntimeGeneration,
    mutation: &DeploymentMutationV1,
) -> Result<bool, RuntimeConvergenceStoreError> {
    let snapshot = persisted.deployment.snapshot();
    let replay_revision = expected_revision
        .next()
        .is_ok_and(|revision| revision == snapshot.revision);
    let replay_fence = snapshot.last_fencing_token == Some(fencing_token)
        && snapshot.runtime_generation == runtime_generation;
    match (&snapshot.phase, mutation) {
        (
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition:
                    RuntimePendingConditionV1::Retryable {
                        failure,
                        attempt,
                        retry_not_before,
                    },
            },
            DeploymentMutationV1::RecordRetryableFailure {
                failure_id,
                kind,
                code,
                message,
                attempt: requested_attempt,
                retry_after,
            },
        ) if failure.failure_id == *failure_id => {
            let stored_delay =
                (*retry_not_before - failure.recorded_at)
                    .to_std()
                    .map_err(|_| {
                        RuntimeConvergenceStoreError::InvalidPersistedState(
                            "runtime retry duration",
                        )
                    })?;
            let requested_delay = PostgresRuntimeConvergence::bounded_duration(
                retry_after.to_owned(),
                adapter.config.maximum_retry_delay,
                "retry delay",
            )?
            .to_std()
            .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("retry delay"))?;
            let exact = replay_revision
                && replay_fence
                && persisted.deployment.controller_lease().is_none()
                && persisted.exact_convergence_attempt()?.started() == Some(*attempt)
                && failure.kind == *kind
                && failure.code == *code
                && failure.message == *message
                && *attempt == *requested_attempt
                && persisted.last_failure_attempt == Some(*attempt)
                && stored_delay == requested_delay;
            if exact {
                Ok(true)
            } else {
                Err(RuntimeConvergenceStoreError::IdempotencyConflict)
            }
        }
        (
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { failure },
            },
            DeploymentMutationV1::RecordBlockedFailure {
                failure_id,
                kind,
                code,
                message,
            },
        ) if failure.failure_id == *failure_id => {
            let exact = replay_revision
                && replay_fence
                && persisted.deployment.controller_lease().is_none()
                && failure.kind == *kind
                && failure.code == *code
                && failure.message == *message
                && persisted.last_failure_attempt
                    == persisted.exact_convergence_attempt()?.started();
            if exact {
                Ok(true)
            } else {
                Err(RuntimeConvergenceStoreError::IdempotencyConflict)
            }
        }
        _ => Ok(false),
    }
}
