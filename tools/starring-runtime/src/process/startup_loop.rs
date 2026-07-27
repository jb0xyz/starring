use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1;
use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::RuntimeClosedRecoveryProcessCleanupFailureV2;

use super::closed::{RuntimeClosedRecoveryProcessV2, RuntimeProcessClosedRecoveryCommitFailureV2};
use super::connected::{
    discord_transition_failure_v1, map_discord_transition_exit_v1,
    RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::execution::RuntimeStartupRecoveryExecutionCompletionV2;
use super::observation::{
    RuntimeProcessStartupRecoveryObservationErrorV2, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryContinueProcessV2,
    RuntimeStartupRecoveryFixedPointProcessV2, RuntimeStartupRecoveryObservationFinalizeFailureV2,
    RuntimeStartupRecoveryObservationProcessOutcomeV2, RuntimeStartupRecoveryObservedProcessV2,
};
use super::readiness::{
    RuntimeProcessRecoveryReadinessTransitionErrorV2, RuntimeRecoveryIterationReadyProcessV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessStartupRecoveryLoopFailureV2 {
    OperationDeadlineElapsed,
    PausedConnection(RuntimeProcessPausedConnectedTransitionFailureV1),
    InvalidForeignFreshRetry,
    StaleLiveRecoveryUnavailable,
    StaleLiveExecution(RuntimeExecutionPersistenceErrorV1),
    StaleLiveExecutionRejected(RuntimeProcessClosedRecoveryCommitFailureV2),
    StaleLiveRetryAfterUnsupported,
    ReservedAwaitingCertificationRecoveryUnavailable,
    ReservedAwaitingCertificationExecution(RuntimeExecutionPersistenceErrorV1),
    ReservedAwaitingCertificationExecutionRejected(RuntimeProcessClosedRecoveryCommitFailureV2),
    ReservedAwaitingCertificationRetryAfterUnsupported,
    SuspendedLocalEffectRecoveryUnavailable,
    PendingRuntimeDrainRecoveryUnavailable,
    ProtocolViolation,
}

impl RuntimeProcessStartupRecoveryLoopFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_process_startup_recovery_loop_operation_deadline_elapsed"
            }
            Self::PausedConnection(error) => error.code(),
            Self::InvalidForeignFreshRetry => {
                "runtime_process_startup_recovery_loop_invalid_foreign_fresh_retry"
            }
            Self::StaleLiveRecoveryUnavailable => {
                "runtime_process_startup_recovery_loop_stale_live_recovery_unavailable"
            }
            Self::StaleLiveExecution(error) => runtime_execution_error_code_v2(error),
            Self::StaleLiveExecutionRejected(error) => error.code(),
            Self::StaleLiveRetryAfterUnsupported => {
                "runtime_process_startup_recovery_loop_stale_live_retry_after_unsupported"
            }
            Self::ReservedAwaitingCertificationRecoveryUnavailable => {
                "runtime_process_startup_recovery_loop_reserved_awaiting_certification_recovery_unavailable"
            }
            Self::ReservedAwaitingCertificationExecution(error) => {
                runtime_execution_error_code_v2(error)
            }
            Self::ReservedAwaitingCertificationExecutionRejected(error) => error.code(),
            Self::ReservedAwaitingCertificationRetryAfterUnsupported => {
                "runtime_process_startup_recovery_loop_reserved_awaiting_certification_retry_after_unsupported"
            }
            Self::SuspendedLocalEffectRecoveryUnavailable => {
                "runtime_process_startup_recovery_loop_suspended_local_effect_recovery_unavailable"
            }
            Self::PendingRuntimeDrainRecoveryUnavailable => {
                "runtime_process_startup_recovery_loop_pending_runtime_drain_recovery_unavailable"
            }
            Self::ProtocolViolation => {
                "runtime_process_startup_recovery_loop_protocol_violation"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::OperationDeadlineElapsed
            | Self::PausedConnection(_)
            | Self::InvalidForeignFreshRetry
            | Self::StaleLiveRecoveryUnavailable
            | Self::StaleLiveExecution(_)
            | Self::StaleLiveExecutionRejected(_)
            | Self::StaleLiveRetryAfterUnsupported
            | Self::ReservedAwaitingCertificationRecoveryUnavailable
            | Self::ReservedAwaitingCertificationExecution(_)
            | Self::ReservedAwaitingCertificationExecutionRejected(_)
            | Self::ReservedAwaitingCertificationRetryAfterUnsupported
            | Self::SuspendedLocalEffectRecoveryUnavailable
            | Self::PendingRuntimeDrainRecoveryUnavailable
            | Self::ProtocolViolation => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessStartupRecoveryLoopErrorV2 {
    #[error("runtime process startup recovery loop transition failed")]
    Transition(RuntimeProcessStartupRecoveryLoopFailureV2),
    #[error("runtime process startup recovery loop transition cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessStartupRecoveryLoopFailureV2,
        cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2,
    },
    #[error("runtime process startup recovery loop observation failed")]
    Observation(RuntimeProcessStartupRecoveryObservationErrorV2),
    #[error("runtime process startup recovery loop readiness refresh failed")]
    Readiness(RuntimeProcessRecoveryReadinessTransitionErrorV2),
}

impl RuntimeProcessStartupRecoveryLoopErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_startup_recovery_loop_cleanup_after_transition"
            }
            Self::Observation(error) => error.code(),
            Self::Readiness(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Transition(transition) => transition.context(),
            Self::CleanupAfterTransition { .. } => None,
            Self::Observation(error) => error.context(),
            Self::Readiness(error) => error.context(),
        }
    }

    pub const fn cleanup_class(self) -> bool {
        matches!(
            self,
            Self::CleanupAfterTransition { .. }
                | Self::Observation(
                    RuntimeProcessStartupRecoveryObservationErrorV2::CleanupAfterTransition { .. }
                )
                | Self::Readiness(
                    RuntimeProcessRecoveryReadinessTransitionErrorV2::CleanupAfterTransition { .. }
                )
        )
    }
}

const fn runtime_execution_error_code_v2(
    error: RuntimeExecutionPersistenceErrorV1,
) -> &'static str {
    match error {
        RuntimeExecutionPersistenceErrorV1::InvalidInput => "runtime_execution_invalid_input",
        RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch => {
            "runtime_execution_database_authority_mismatch"
        }
        RuntimeExecutionPersistenceErrorV1::OwnershipLost => "runtime_execution_ownership_lost",
        RuntimeExecutionPersistenceErrorV1::AuthorityChanged => {
            "runtime_execution_authority_changed"
        }
        RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt => {
            "runtime_execution_persistence_corrupt"
        }
        RuntimeExecutionPersistenceErrorV1::RetryNotReady => "runtime_execution_retry_not_ready",
        RuntimeExecutionPersistenceErrorV1::Superseded => "runtime_execution_superseded",
        RuntimeExecutionPersistenceErrorV1::Timeout => "runtime_execution_timeout",
        RuntimeExecutionPersistenceErrorV1::Concurrency => "runtime_execution_concurrency",
        RuntimeExecutionPersistenceErrorV1::Unavailable => "runtime_execution_unavailable",
        RuntimeExecutionPersistenceErrorV1::DatabaseFailure => "runtime_execution_database_failure",
        RuntimeExecutionPersistenceErrorV1::Indeterminate => "runtime_execution_indeterminate",
        RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous => {
            "runtime_execution_observation_ambiguous"
        }
        _ => "runtime_execution_unknown",
    }
}

impl Debug for RuntimeProcessStartupRecoveryLoopErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessStartupRecoveryLoopErrorV2(<redacted>)")
    }
}

struct RuntimeForeignFreshWaitCompletionV2 {
    retry_after: Duration,
}

struct RuntimeForeignFreshRearmFailureV2 {
    process: RuntimeStartupRecoveryContinueProcessV2,
    transition: RuntimeProcessStartupRecoveryLoopFailureV2,
}

enum RuntimeStartupRecoveryLoopIterationOutcomeV2<Continue, FixedPoint> {
    Continue(Continue),
    FixedPoint(FixedPoint),
}

type RuntimeStartupRecoveryBorrowedStepFutureV2<'a, Output, Failure> =
    Pin<Box<dyn Future<Output = Result<Output, Failure>> + 'a>>;

trait RuntimeStartupRecoveryLoopReadyStepV2: Sized {
    type Observed;
    type Continue: RuntimeStartupRecoveryLoopContinueStepV2<Ready = Self, Error = Self::Error>;
    type FixedPoint;
    type ObservationFailure;
    type FinalizeFailure;
    type Error;

    fn observe_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::Observed, Self::ObservationFailure>;

    async fn cleanup_after_observation_failure_v2(
        self,
        failure: Self::ObservationFailure,
    ) -> Self::Error;

    fn finalize_observation_v2(
        self,
        observed: Self::Observed,
    ) -> Result<
        RuntimeStartupRecoveryLoopIterationOutcomeV2<Self::Continue, Self::FixedPoint>,
        Self::FinalizeFailure,
    >;

    async fn cleanup_after_finalize_failure_v2(failure: Self::FinalizeFailure) -> Self::Error;
}

trait RuntimeStartupRecoveryLoopContinueStepV2: Sized {
    type Ready;
    type WaitCompletion;
    type WaitFailure;
    type RecoveryCompletion;
    type RecoveryFailure;
    type Error;

    fn continuation_v2(&self) -> RuntimeStartupRecoveryContinuationV2;

    fn wait_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::WaitCompletion, Self::WaitFailure>;

    async fn cleanup_after_wait_failure_v2(self, failure: Self::WaitFailure) -> Self::Error;

    fn execute_recovery_in_place_v2(
        &mut self,
        class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<
        '_,
        Self::RecoveryCompletion,
        Self::RecoveryFailure,
    >;

    async fn cleanup_after_recovery_failure_v2(self, failure: Self::RecoveryFailure)
        -> Self::Error;

    async fn into_next_ready_after_recovery_v2(
        self,
        completion: Self::RecoveryCompletion,
    ) -> Result<Self::Ready, Self::Error>;

    async fn into_next_ready_v2(
        self,
        completion: Self::WaitCompletion,
    ) -> Result<Self::Ready, Self::Error>;
}

async fn drive_startup_recovery_loop_v2<Ready>(
    mut ready: Ready,
) -> Result<Ready::FixedPoint, Ready::Error>
where
    Ready: RuntimeStartupRecoveryLoopReadyStepV2,
{
    loop {
        let observed = match ready.observe_in_place_v2().await {
            Ok(observed) => observed,
            Err(failure) => {
                return Err(ready.cleanup_after_observation_failure_v2(failure).await);
            }
        };
        let outcome = match ready.finalize_observation_v2(observed) {
            Ok(outcome) => outcome,
            Err(failure) => {
                return Err(Ready::cleanup_after_finalize_failure_v2(failure).await);
            }
        };
        match outcome {
            RuntimeStartupRecoveryLoopIterationOutcomeV2::FixedPoint(fixed_point) => {
                return Ok(fixed_point);
            }
            RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(mut process) => {
                match process.continuation_v2() {
                    RuntimeStartupRecoveryContinuationV2::Recover(class) => {
                        let completion = match process.execute_recovery_in_place_v2(class).await {
                            Ok(completion) => completion,
                            Err(failure) => {
                                return Err(process
                                    .cleanup_after_recovery_failure_v2(failure)
                                    .await);
                            }
                        };
                        ready = process
                            .into_next_ready_after_recovery_v2(completion)
                            .await?;
                    }
                    RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh { .. } => {
                        let completion = match process.wait_in_place_v2().await {
                            Ok(completion) => completion,
                            Err(failure) => {
                                return Err(process.cleanup_after_wait_failure_v2(failure).await);
                            }
                        };
                        ready = process.into_next_ready_v2(completion).await?;
                    }
                }
            }
        }
    }
}

impl RuntimeStartupRecoveryLoopReadyStepV2 for RuntimeRecoveryIterationReadyProcessV2 {
    type Observed = RuntimeStartupRecoveryObservedProcessV2;
    type Continue = RuntimeStartupRecoveryContinueProcessV2;
    type FixedPoint = RuntimeStartupRecoveryFixedPointProcessV2;
    type ObservationFailure = super::observation::RuntimeProcessStartupRecoveryObservationFailureV2;
    type FinalizeFailure = Box<RuntimeStartupRecoveryObservationFinalizeFailureV2>;
    type Error = RuntimeProcessStartupRecoveryLoopErrorV2;

    fn observe_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::Observed, Self::ObservationFailure>
    {
        let observer = self.foundation.databases.execution().clone();
        Box::pin(async move { self.observe_startup_recovery_once_v2(&observer).await })
    }

    async fn cleanup_after_observation_failure_v2(
        self,
        failure: Self::ObservationFailure,
    ) -> Self::Error {
        RuntimeProcessStartupRecoveryLoopErrorV2::Observation(
            self.cleanup_after_startup_recovery_observation_failure_v2(failure)
                .await,
        )
    }

    fn finalize_observation_v2(
        self,
        observed: Self::Observed,
    ) -> Result<
        RuntimeStartupRecoveryLoopIterationOutcomeV2<Self::Continue, Self::FixedPoint>,
        Self::FinalizeFailure,
    > {
        self.into_startup_recovery_observation_outcome_v2(observed)
            .map(|outcome| match outcome {
                RuntimeStartupRecoveryObservationProcessOutcomeV2::Continue(process) => {
                    RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(process)
                }
                RuntimeStartupRecoveryObservationProcessOutcomeV2::FixedPoint(process) => {
                    RuntimeStartupRecoveryLoopIterationOutcomeV2::FixedPoint(process)
                }
            })
    }

    async fn cleanup_after_finalize_failure_v2(failure: Self::FinalizeFailure) -> Self::Error {
        RuntimeProcessStartupRecoveryLoopErrorV2::Observation(failure.cleanup().await)
    }
}

impl RuntimeStartupRecoveryLoopContinueStepV2 for RuntimeStartupRecoveryContinueProcessV2 {
    type Ready = RuntimeRecoveryIterationReadyProcessV2;
    type WaitCompletion = RuntimeForeignFreshWaitCompletionV2;
    type WaitFailure = RuntimeProcessStartupRecoveryLoopFailureV2;
    type RecoveryCompletion = RuntimeStartupRecoveryExecutionCompletionV2;
    type RecoveryFailure = RuntimeProcessStartupRecoveryLoopFailureV2;
    type Error = RuntimeProcessStartupRecoveryLoopErrorV2;

    fn continuation_v2(&self) -> RuntimeStartupRecoveryContinuationV2 {
        RuntimeStartupRecoveryContinueProcessV2::continuation_v2(self)
    }

    fn wait_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::WaitCompletion, Self::WaitFailure>
    {
        Box::pin(self.wait_for_foreign_fresh_in_place_v2())
    }

    async fn cleanup_after_wait_failure_v2(self, failure: Self::WaitFailure) -> Self::Error {
        finish_startup_recovery_loop_transition_v2(failure, self.shutdown().await)
    }

    fn execute_recovery_in_place_v2(
        &mut self,
        class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<
        '_,
        Self::RecoveryCompletion,
        Self::RecoveryFailure,
    > {
        Box::pin(self.execute_startup_recovery_in_place_v2(class))
    }

    async fn cleanup_after_recovery_failure_v2(
        self,
        failure: Self::RecoveryFailure,
    ) -> Self::Error {
        self.session.invalidate_startup_recovery_execution_v2();
        finish_startup_recovery_loop_transition_v2(failure, self.shutdown().await)
    }

    async fn into_next_ready_after_recovery_v2(
        self,
        _completion: Self::RecoveryCompletion,
    ) -> Result<Self::Ready, Self::Error> {
        let Self {
            discord,
            foundation,
            session,
            continuation: _,
        } = self;
        RuntimeClosedRecoveryProcessV2 {
            discord,
            foundation,
            session,
        }
        .into_recovery_iteration_ready_v2()
        .await
        .map_err(RuntimeProcessStartupRecoveryLoopErrorV2::Readiness)
    }

    async fn into_next_ready_v2(
        self,
        completion: Self::WaitCompletion,
    ) -> Result<Self::Ready, Self::Error> {
        let closed = match self.into_closed_recovery_after_foreign_fresh_v2(completion) {
            Ok(closed) => closed,
            Err(failure) => {
                let RuntimeForeignFreshRearmFailureV2 {
                    process,
                    transition,
                } = *failure;
                return Err(finish_startup_recovery_loop_transition_v2(
                    transition,
                    process.shutdown().await,
                ));
            }
        };
        closed
            .into_recovery_iteration_ready_v2()
            .await
            .map_err(RuntimeProcessStartupRecoveryLoopErrorV2::Readiness)
    }
}

impl RuntimeRecoveryIterationReadyProcessV2 {
    pub(crate) async fn into_startup_recovery_fixed_point_v2(
        self,
    ) -> Result<RuntimeStartupRecoveryFixedPointProcessV2, RuntimeProcessStartupRecoveryLoopErrorV2>
    {
        drive_startup_recovery_loop_v2(self).await
    }
}

impl RuntimeStartupRecoveryContinueProcessV2 {
    async fn wait_for_foreign_fresh_in_place_v2(
        &mut self,
    ) -> Result<RuntimeForeignFreshWaitCompletionV2, RuntimeProcessStartupRecoveryLoopFailureV2>
    {
        let retry_after = match self.continuation {
            RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh { retry_after } => {
                retry_after
            }
            RuntimeStartupRecoveryContinuationV2::Recover(_) => {
                return Err(RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation);
            }
        };
        if retry_after.is_zero() {
            return Err(RuntimeProcessStartupRecoveryLoopFailureV2::InvalidForeignFreshRetry);
        }
        if let Some(transition) = current_foreign_fresh_wait_transition_v2(self) {
            return Err(transition);
        }
        let now = Instant::now();
        let retry_at = now
            .checked_add(retry_after)
            .ok_or(RuntimeProcessStartupRecoveryLoopFailureV2::InvalidForeignFreshRetry)?;
        let operation_cutoff = self.foundation.startup_budget.operation_cutoff();
        let owner_safety_deadline = self.session.owner_safety_deadline_v2();
        let wait_cutoff = operation_cutoff.min(owner_safety_deadline);
        if now >= wait_cutoff {
            return Err(classify_foreign_fresh_wait_deadline_v2(
                operation_cutoff,
                owner_safety_deadline,
            ));
        }
        let deadline_failure =
            classify_foreign_fresh_wait_deadline_v2(operation_cutoff, owner_safety_deadline);
        let owner_terminal = self.session.owner_terminal_observation_v2();
        let discord_terminal =
            async { map_discord_transition_exit_v1(self.discord.wait_terminal().await.exit()) };
        let owner_terminal = async {
            let _exit = owner_terminal.await;
        };
        await_foreign_fresh_retry_v2(
            deadline_failure,
            sleep_until(TokioInstant::from_std(wait_cutoff)),
            discord_terminal,
            owner_terminal,
            sleep_until(TokioInstant::from_std(retry_at)),
        )
        .await?;
        if let Some(transition) = current_foreign_fresh_wait_transition_v2(self) {
            return Err(transition);
        }
        Ok(RuntimeForeignFreshWaitCompletionV2 { retry_after })
    }

    fn into_closed_recovery_after_foreign_fresh_v2(
        self,
        completion: RuntimeForeignFreshWaitCompletionV2,
    ) -> Result<RuntimeClosedRecoveryProcessV2, Box<RuntimeForeignFreshRearmFailureV2>> {
        if self.continuation
            != (RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                retry_after: completion.retry_after,
            })
        {
            return Err(Box::new(RuntimeForeignFreshRearmFailureV2 {
                process: self,
                transition: RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
            }));
        }
        let Self {
            discord,
            foundation,
            session,
            continuation: _,
        } = self;
        Ok(RuntimeClosedRecoveryProcessV2 {
            discord,
            foundation,
            session,
        })
    }
}

fn current_foreign_fresh_wait_transition_v2(
    process: &RuntimeStartupRecoveryContinueProcessV2,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    classify_current_foreign_fresh_wait_transition_v2(
        Instant::now(),
        process.foundation.startup_budget.operation_cutoff(),
        process.session.owner_safety_deadline_v2(),
        discord_transition_failure_v1(&process.discord),
        process.session.owner_terminal_status_v2().is_some(),
    )
}

fn classify_current_foreign_fresh_wait_transition_v2(
    now: Instant,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
    discord: Option<RuntimeProcessPausedConnectedTransitionFailureV1>,
    owner_terminal: bool,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    if now >= operation_cutoff.min(owner_safety_deadline) {
        return Some(classify_foreign_fresh_wait_deadline_v2(
            operation_cutoff,
            owner_safety_deadline,
        ));
    }
    if let Some(error) = discord {
        return Some(RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(error));
    }
    owner_terminal.then_some(
        RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        ),
    )
}

fn classify_foreign_fresh_wait_deadline_v2(
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    if owner_safety_deadline <= operation_cutoff {
        RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    } else {
        RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
    }
}

async fn await_foreign_fresh_retry_v2<Deadline, DiscordTerminal, OwnerTerminal, Retry>(
    deadline_failure: RuntimeProcessStartupRecoveryLoopFailureV2,
    deadline: Deadline,
    discord_terminal: DiscordTerminal,
    owner_terminal: OwnerTerminal,
    retry: Retry,
) -> Result<(), RuntimeProcessStartupRecoveryLoopFailureV2>
where
    Deadline: Future<Output = ()>,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
    OwnerTerminal: Future<Output = ()>,
    Retry: Future<Output = ()>,
{
    tokio::pin!(deadline);
    tokio::pin!(discord_terminal);
    tokio::pin!(owner_terminal);
    tokio::pin!(retry);
    tokio::select! {
        biased;
        () = &mut deadline => Err(deadline_failure),
        transition = &mut discord_terminal => {
            Err(RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(transition))
        }
        () = &mut owner_terminal => {
            Err(RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ))
        }
        () = &mut retry => Ok(()),
    }
}

pub(super) fn unavailable_recovery_failure_v2(
    class: RuntimeStartupRecoveryClassV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveRecoveryUnavailable
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationRecoveryUnavailable
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectRecoveryUnavailable
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable
        }
    }
}

fn finish_startup_recovery_loop_transition_v2(
    transition: RuntimeProcessStartupRecoveryLoopFailureV2,
    cleanup: Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2>,
) -> RuntimeProcessStartupRecoveryLoopErrorV2 {
    match cleanup {
        Ok(()) => RuntimeProcessStartupRecoveryLoopErrorV2::Transition(transition),
        Err(cleanup) => RuntimeProcessStartupRecoveryLoopErrorV2::CleanupAfterTransition {
            transition,
            cleanup,
        },
    }
}

#[cfg(test)]
#[path = "startup_loop_tests.rs"]
mod tests;
