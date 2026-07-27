use std::future::Future;
use std::time::Instant;

use automation_runtime_worker::{
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
    RuntimeStartupRecoveryExecutionPortV2, RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
};
use tokio::time::{sleep_until, Instant as TokioInstant};

use super::connected::{
    discord_transition_failure_v1, map_discord_transition_exit_v1,
    RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::observation::RuntimeStartupRecoveryContinueProcessV2;
use super::startup_loop::RuntimeProcessStartupRecoveryLoopFailureV2;

pub(super) struct RuntimeStartupRecoveryExecutionCompletionV2;

enum RuntimeStartupRecoveryExecutionAwaitFailureV2<E> {
    Transition(RuntimeProcessStartupRecoveryLoopFailureV2),
    Database(E),
}

impl RuntimeStartupRecoveryContinueProcessV2 {
    pub(super) async fn execute_startup_recovery_in_place_v2(
        &mut self,
        class: RuntimeStartupRecoveryClassV2,
    ) -> Result<
        RuntimeStartupRecoveryExecutionCompletionV2,
        RuntimeProcessStartupRecoveryLoopFailureV2,
    > {
        if !matches!(
            class,
            RuntimeStartupRecoveryClassV2::StaleLive
                | RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
                | RuntimeStartupRecoveryClassV2::SuspendedLocalEffect
        ) {
            return Err(super::startup_loop::unavailable_recovery_failure_v2(class));
        }
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            return Err(transition);
        }
        let authorization = match self.session.begin_startup_recovery_execution_v2(
            RuntimeStartupRecoveryContinuationV2::Recover(class),
        ) {
            Ok(authorization) => authorization,
            Err(error) => {
                let transition = prefer_current_startup_recovery_execution_failure_v2(
                    current_startup_recovery_execution_transition_v2(self),
                    || startup_recovery_execution_rejected_v2(class, error.into()),
                );
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
        };
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(transition);
        }
        let operation_cutoff = self.foundation.startup_budget.operation_cutoff();
        let owner_safety_deadline = self.session.owner_safety_deadline_v2();
        let execution_cutoff = operation_cutoff.min(owner_safety_deadline);
        let deadline_failure = classify_startup_recovery_execution_deadline_v2(
            operation_cutoff,
            owner_safety_deadline,
        );
        let owner_terminal = self.session.owner_terminal_observation_v2();
        let discord_terminal =
            async { map_discord_transition_exit_v1(self.discord.wait_terminal().await.exit()) };
        let owner_terminal = async {
            let _exit = owner_terminal.await;
        };
        let database = self
            .foundation
            .databases
            .execution()
            .execute_startup_recovery(authorization, execution_cutoff);
        let completed = match await_startup_recovery_execution_v2(
            deadline_failure,
            sleep_until(TokioInstant::from_std(execution_cutoff)),
            discord_terminal,
            owner_terminal,
            database,
        )
        .await
        {
            Ok(completed) => completed,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(transition)) => {
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error)) => {
                let transition = prefer_current_startup_recovery_execution_failure_v2(
                    current_startup_recovery_execution_transition_v2(self),
                    || startup_recovery_execution_database_failure_v2(class, error),
                );
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
        };
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(transition);
        }
        let accepted = match self
            .session
            .complete_startup_recovery_execution_v2(completed)
        {
            Ok(accepted) => accepted,
            Err(error) => {
                let transition = prefer_current_startup_recovery_execution_failure_v2(
                    current_startup_recovery_execution_transition_v2(self),
                    || startup_recovery_execution_rejected_v2(class, error.into()),
                );
                self.session.invalidate_startup_recovery_execution_v2();
                return Err(transition);
            }
        };
        if let Some(transition) = current_startup_recovery_execution_transition_v2(self) {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(transition);
        }
        if accepted.class() != class {
            self.session.invalidate_startup_recovery_execution_v2();
            return Err(startup_recovery_execution_rejected_v2(
                class,
                crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation,
            ));
        }
        match accepted.outcome() {
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
            | RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate => {
                Ok(RuntimeStartupRecoveryExecutionCompletionV2)
            }
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { .. } => {
                self.session.invalidate_startup_recovery_execution_v2();
                Err(startup_recovery_execution_retry_after_unsupported_v2(class))
            }
        }
    }
}

fn startup_recovery_execution_database_failure_v2(
    class: RuntimeStartupRecoveryClassV2,
    error: automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecution(error)
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecution(
                error,
            )
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecution(error)
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            super::startup_loop::unavailable_recovery_failure_v2(class)
        }
    }
}

fn startup_recovery_execution_rejected_v2(
    class: RuntimeStartupRecoveryClassV2,
    error: crate::RuntimeProcessClosedRecoveryCommitFailureV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecutionRejected(error)
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecutionRejected(
                error,
            )
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecutionRejected(error)
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            super::startup_loop::unavailable_recovery_failure_v2(class)
        }
    }
}

fn startup_recovery_execution_retry_after_unsupported_v2(
    class: RuntimeStartupRecoveryClassV2,
) -> RuntimeProcessStartupRecoveryLoopFailureV2 {
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveRetryAfterUnsupported
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationRetryAfterUnsupported
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectRetryAfterUnsupported
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            super::startup_loop::unavailable_recovery_failure_v2(class)
        }
    }
}

fn prefer_current_startup_recovery_execution_failure_v2<Fallback>(
    current: Option<RuntimeProcessStartupRecoveryLoopFailureV2>,
    fallback: Fallback,
) -> RuntimeProcessStartupRecoveryLoopFailureV2
where
    Fallback: FnOnce() -> RuntimeProcessStartupRecoveryLoopFailureV2,
{
    current.unwrap_or_else(fallback)
}

fn current_startup_recovery_execution_transition_v2(
    process: &RuntimeStartupRecoveryContinueProcessV2,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    classify_current_startup_recovery_execution_transition_v2(
        Instant::now(),
        process.foundation.startup_budget.operation_cutoff(),
        process.session.owner_safety_deadline_v2(),
        discord_transition_failure_v1(&process.discord),
        process.session.owner_terminal_status_v2().is_some(),
    )
}

fn classify_current_startup_recovery_execution_transition_v2(
    now: Instant,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
    discord: Option<RuntimeProcessPausedConnectedTransitionFailureV1>,
    owner_terminal: bool,
) -> Option<RuntimeProcessStartupRecoveryLoopFailureV2> {
    if now >= operation_cutoff.min(owner_safety_deadline) {
        return Some(classify_startup_recovery_execution_deadline_v2(
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

fn classify_startup_recovery_execution_deadline_v2(
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

async fn await_startup_recovery_execution_v2<
    Deadline,
    DiscordTerminal,
    OwnerTerminal,
    Execution,
    Completed,
    E,
>(
    deadline_failure: RuntimeProcessStartupRecoveryLoopFailureV2,
    deadline: Deadline,
    discord_terminal: DiscordTerminal,
    owner_terminal: OwnerTerminal,
    execution: Execution,
) -> Result<Completed, RuntimeStartupRecoveryExecutionAwaitFailureV2<E>>
where
    Deadline: Future<Output = ()>,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
    OwnerTerminal: Future<Output = ()>,
    Execution: Future<Output = Result<Completed, E>>,
{
    tokio::pin!(deadline);
    tokio::pin!(discord_terminal);
    tokio::pin!(owner_terminal);
    tokio::pin!(execution);
    tokio::select! {
        biased;
        () = &mut deadline => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(deadline_failure))
        }
        transition = &mut discord_terminal => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(transition),
            ))
        }
        () = &mut owner_terminal => {
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                ),
            ))
        }
        result = &mut execution => {
            result.map_err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Database)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, ready};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use super::*;

    struct TrackedPendingExecutionV2 {
        polled: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    }

    impl Future for TrackedPendingExecutionV2 {
        type Output = Result<(), ()>;

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            self.polled.store(true, Ordering::Release);
            Poll::Pending
        }
    }

    impl Drop for TrackedPendingExecutionV2 {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn execution_wait_priority_is_cutoff_then_discord_then_owner_then_database() {
        let deadline = RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed;
        let all_ready = await_startup_recovery_execution_v2(
            deadline,
            ready(()),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
            ready(()),
            ready(Ok::<_, ()>(())),
        )
        .await;
        assert!(matches!(
            all_ready,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
            ))
        ));

        let discord_ready = await_startup_recovery_execution_v2(
            deadline,
            pending::<()>(),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
            ready(()),
            ready(Ok::<_, ()>(())),
        )
        .await;
        assert!(matches!(
            discord_ready,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated
                )
            ))
        ));

        let owner_ready = await_startup_recovery_execution_v2(
            deadline,
            pending::<()>(),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            ready(()),
            ready(Ok::<_, ()>(())),
        )
        .await;
        assert!(matches!(
            owner_ready,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated
                )
            ))
        ));

        let database_ready = await_startup_recovery_execution_v2(
            deadline,
            pending::<()>(),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            pending::<()>(),
            ready(Ok::<_, ()>(())),
        )
        .await;
        assert!(matches!(database_ready, Ok(())));

        let database_error = await_startup_recovery_execution_v2(
            deadline,
            ready(()),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            pending::<()>(),
            ready(Err::<(), _>("database")),
        )
        .await;
        assert!(matches!(
            database_error,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
            ))
        ));
    }

    #[tokio::test]
    async fn interrupted_execution_future_is_dropped() {
        let polled = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let execution = TrackedPendingExecutionV2 {
            polled: polled.clone(),
            dropped: dropped.clone(),
        };
        let result = await_startup_recovery_execution_v2(
            RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
            ready(()),
            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
            pending::<()>(),
            execution,
        )
        .await;

        assert!(matches!(
            result,
            Err(RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(
                RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
            ))
        ));
        assert!(!polled.load(Ordering::Acquire));
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn current_execution_state_and_deadline_cause_are_deterministic() {
        let now = Instant::now();
        let discord = Some(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated);
        assert_eq!(
            classify_current_startup_recovery_execution_transition_v2(
                now,
                now,
                now + Duration::from_secs(1),
                discord,
                true,
            ),
            Some(RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed)
        );
        assert_eq!(
            classify_current_startup_recovery_execution_transition_v2(
                now,
                now + Duration::from_secs(1),
                now,
                discord,
                true,
            ),
            Some(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                )
            )
        );
        assert_eq!(
            classify_current_startup_recovery_execution_transition_v2(
                now,
                now + Duration::from_secs(2),
                now + Duration::from_secs(1),
                discord,
                true,
            ),
            Some(
                RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                )
            )
        );
        assert_eq!(
            prefer_current_startup_recovery_execution_failure_v2(
                Some(RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed),
                || RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecutionRejected(
                    crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation,
                ),
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed
        );
    }

    #[test]
    fn supported_recovery_classes_preserve_distinct_execution_failures() {
        let database_error =
            automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let protocol_error =
            crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayProtocolViolation;

        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::StaleLive,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecution(database_error)
        );
        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecution(
                database_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_rejected_v2(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
                protocol_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecutionRejected(
                protocol_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_retry_after_unsupported_v2(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationRetryAfterUnsupported
        );
        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecution(
                database_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_rejected_v2(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
                protocol_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecutionRejected(
                protocol_error,
            )
        );
        assert_eq!(
            startup_recovery_execution_retry_after_unsupported_v2(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectRetryAfterUnsupported
        );
        assert_eq!(
            startup_recovery_execution_database_failure_v2(
                RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
                database_error,
            ),
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable
        );
    }
}
