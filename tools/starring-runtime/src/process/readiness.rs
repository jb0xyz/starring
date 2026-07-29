use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::closed_recovery::{
    RuntimeClosedRecoveryCommitErrorV2, RuntimeClosedRecoveryReadinessRefreshErrorV2,
    RuntimeClosedRecoveryReadyIterationV2, RuntimeClosedRecoverySessionV2,
};
use crate::discord::RuntimeDiscordGatewaySupervisorV1;
use crate::{
    RuntimeClosedRecoveryProcessCleanupFailureV2, RuntimeDatabaseCompositionErrorV1,
    RuntimePausedConnectedProcessShutdownErrorV1, RuntimeProcessGatewayOwnerCommitFailureV2,
    RuntimeRegistryRecoveryObservationErrorV1,
};

use super::closed::{shutdown_committed_recovery_v2, RuntimeClosedRecoveryProcessV2};
use super::connected::{
    discord_transition_failure_v1, map_discord_transition_exit_v1,
    shutdown_paused_foundation_owner_v1, RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::RuntimeProcessFoundationV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessRecoveryReadinessFailureV2 {
    DeadlineElapsed,
    Database(RuntimeDatabaseCompositionErrorV1),
    GatewayObservation(crate::RuntimeGatewayReadyObservationErrorV1),
    GatewayCoordinator,
    GatewayProtocolViolation,
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    OwnerLifetime(RuntimeProcessGatewayOwnerCommitFailureV2),
}

impl From<RuntimeClosedRecoveryReadinessRefreshErrorV2>
    for RuntimeProcessRecoveryReadinessFailureV2
{
    fn from(error: RuntimeClosedRecoveryReadinessRefreshErrorV2) -> Self {
        match error {
            RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed => Self::DeadlineElapsed,
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Database(error) => Self::Database(error),
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(error) => {
                map_gateway_readiness_failure_v2(error)
            }
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Registry(error) => Self::Registry(error),
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Owner(error) => {
                Self::OwnerLifetime(error.into())
            }
        }
    }
}

impl From<RuntimeClosedRecoveryCommitErrorV2> for RuntimeProcessRecoveryReadinessFailureV2 {
    fn from(error: RuntimeClosedRecoveryCommitErrorV2) -> Self {
        match error {
            RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed => Self::DeadlineElapsed,
            RuntimeClosedRecoveryCommitErrorV2::Gateway(error) => {
                map_gateway_readiness_failure_v2(error)
            }
            RuntimeClosedRecoveryCommitErrorV2::Registry(error) => Self::Registry(error),
            RuntimeClosedRecoveryCommitErrorV2::Owner(error) => Self::OwnerLifetime(error.into()),
        }
    }
}

fn map_gateway_readiness_failure_v2(
    error: crate::gateway::RuntimeGatewayRecoverySectionErrorV2,
) -> RuntimeProcessRecoveryReadinessFailureV2 {
    match error {
        crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(error) => {
            RuntimeProcessRecoveryReadinessFailureV2::GatewayObservation(error)
        }
        crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Coordinator(_) => {
            RuntimeProcessRecoveryReadinessFailureV2::GatewayCoordinator
        }
        crate::gateway::RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation => {
            RuntimeProcessRecoveryReadinessFailureV2::GatewayProtocolViolation
        }
    }
}

impl RuntimeProcessRecoveryReadinessFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::DeadlineElapsed => "runtime_closed_recovery_readiness_refresh_deadline_elapsed",
            Self::Database(error) => error.code(),
            Self::GatewayObservation(error) => error.code(),
            Self::GatewayCoordinator => {
                "runtime_closed_recovery_readiness_refresh_gateway_coordinator"
            }
            Self::GatewayProtocolViolation => {
                "runtime_closed_recovery_readiness_refresh_gateway_protocol_violation"
            }
            Self::Registry(error) => error.code(),
            Self::OwnerLifetime(error) => match error {
                RuntimeProcessGatewayOwnerCommitFailureV2::SafetyElapsed => {
                    "runtime_closed_recovery_readiness_owner_safety_elapsed"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::OwnerReceiptMismatch => {
                    "runtime_closed_recovery_readiness_owner_receipt_mismatch"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::ProtocolViolation => {
                    "runtime_closed_recovery_readiness_owner_protocol_violation"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::SupervisorUnavailable => {
                    "runtime_closed_recovery_readiness_owner_supervisor_unavailable"
                }
            },
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Database(error) => error.context(),
            Self::DeadlineElapsed
            | Self::GatewayObservation(_)
            | Self::GatewayCoordinator
            | Self::GatewayProtocolViolation
            | Self::Registry(_)
            | Self::OwnerLifetime(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessRecoveryReadinessTransitionFailureV2 {
    OperationDeadlineElapsed,
    PausedConnection(RuntimeProcessPausedConnectedTransitionFailureV1),
    Readiness(RuntimeProcessRecoveryReadinessFailureV2),
}

impl RuntimeProcessRecoveryReadinessTransitionFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_process_recovery_readiness_operation_deadline_elapsed"
            }
            Self::PausedConnection(error) => error.code(),
            Self::Readiness(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Readiness(error) => error.context(),
            Self::OperationDeadlineElapsed | Self::PausedConnection(_) => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessRecoveryReadinessTransitionErrorV2 {
    #[error("runtime process recovery-readiness transition failed")]
    Transition(RuntimeProcessRecoveryReadinessTransitionFailureV2),
    #[error("runtime process recovery-readiness transition cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessRecoveryReadinessTransitionFailureV2,
        cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2,
    },
}

impl RuntimeProcessRecoveryReadinessTransitionErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_recovery_readiness_transition_cleanup"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Transition(transition) => transition.context(),
            Self::CleanupAfterTransition { .. } => None,
        }
    }
}

impl Debug for RuntimeProcessRecoveryReadinessTransitionErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessRecoveryReadinessTransitionErrorV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRecoveryIterationReadyProcessV2 {
    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,
    pub(super) foundation: RuntimeProcessFoundationV1,
    pub(super) iteration: RuntimeClosedRecoveryReadyIterationV2,
}

impl RuntimeClosedRecoveryProcessV2 {
    pub(crate) async fn into_recovery_iteration_ready_v2(
        self,
    ) -> Result<
        RuntimeRecoveryIterationReadyProcessV2,
        RuntimeProcessRecoveryReadinessTransitionErrorV2,
    > {
        let RuntimeClosedRecoveryProcessV2 {
            mut discord,
            foundation,
            mut session,
        } = self;
        if let Err(transition) = require_current_closed_recovery_v2(&foundation, &discord, &session)
        {
            let cleanup = shutdown_committed_recovery_v2(foundation, discord, session).await;
            return Err(finish_readiness_transition_v2(transition, cleanup));
        }
        let readiness_cutoff = session.readiness_cutoff_v2();
        let deadline_failure = classify_readiness_deadline_v2(
            foundation.startup_budget.operation_cutoff(),
            session.owner_safety_deadline_v2(),
        );
        let owner_terminal = session.owner_terminal_observation_v2();
        let refresh = session.refresh_iteration_readiness_in_place_v2(&foundation.databases);
        let discord_terminal = async {
            let transition = map_discord_transition_exit_v1(discord.wait_terminal().await.exit());
            foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
            transition
        };
        let owner_terminal = async {
            let _exit = owner_terminal.await;
            foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
        };
        let mut shutdown = foundation.shutdown_observer_v1();
        let refreshed = await_recovery_readiness_refresh_v2(
            readiness_cutoff,
            deadline_failure,
            refresh,
            discord_terminal,
            owner_terminal,
            async move { shutdown.wait().await },
        )
        .await;
        if let Err(transition) = refreshed {
            let cleanup = shutdown_committed_recovery_v2(foundation, discord, session).await;
            return Err(finish_readiness_transition_v2(transition, cleanup));
        }
        let iteration = match session.try_into_ready_iteration_v2() {
            Ok(iteration) => iteration,
            Err(session) => {
                let transition = RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness(
                    RuntimeProcessRecoveryReadinessFailureV2::GatewayProtocolViolation,
                );
                let cleanup = shutdown_committed_recovery_v2(foundation, discord, *session).await;
                return Err(finish_readiness_transition_v2(transition, cleanup));
            }
        };
        let iteration_validation = iteration
            .revalidate_v2()
            .map_err(RuntimeProcessRecoveryReadinessFailureV2::from)
            .map_err(RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness);
        let final_transition = if let Some(error) = discord_transition_failure_v1(&discord) {
            foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
            Some(RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(error))
        } else if iteration.owner_terminal_status_v2().is_some()
            || Instant::now() >= iteration.owner_safety_deadline_v2()
        {
            foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
            Some(
                RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                ),
            )
        } else if !foundation.startup_budget.operation_is_open() {
            Some(RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed)
        } else {
            iteration_validation.err()
        };
        if let Some(transition) = final_transition {
            let cleanup = shutdown_ready_recovery_v2(foundation, discord, iteration).await;
            return Err(finish_readiness_transition_v2(transition, cleanup));
        }
        Ok(RuntimeRecoveryIterationReadyProcessV2 {
            discord,
            foundation,
            iteration,
        })
    }
}

impl Debug for RuntimeRecoveryIterationReadyProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryIterationReadyProcessV2(<redacted>)")
    }
}

fn require_current_closed_recovery_v2(
    foundation: &RuntimeProcessFoundationV1,
    discord: &RuntimeDiscordGatewaySupervisorV1,
    session: &RuntimeClosedRecoverySessionV2,
) -> Result<(), RuntimeProcessRecoveryReadinessTransitionFailureV2> {
    if !foundation.startup_budget.operation_is_open() {
        return Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed);
    }
    if let Some(error) = discord_transition_failure_v1(discord) {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
        return Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(error));
    }
    if session.owner_terminal_status_v2().is_some()
        || Instant::now() >= session.owner_safety_deadline_v2()
    {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
        return Err(
            RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        );
    }
    session
        .revalidate_v2()
        .map_err(RuntimeProcessRecoveryReadinessFailureV2::from)
        .map_err(RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness)?;
    if session.owner_terminal_status_v2().is_some()
        || Instant::now() >= session.owner_safety_deadline_v2()
    {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
        return Err(
            RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        );
    }
    if let Some(error) = discord_transition_failure_v1(discord) {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
        return Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(error));
    }
    if !foundation.startup_budget.operation_is_open() {
        return Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed);
    }
    Ok(())
}

fn classify_readiness_deadline_v2(
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessRecoveryReadinessTransitionFailureV2 {
    if owner_safety_deadline <= operation_cutoff {
        RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    } else {
        RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed
    }
}

async fn await_recovery_readiness_refresh_v2<Refresh, DiscordTerminal, OwnerTerminal, Shutdown>(
    readiness_cutoff: Instant,
    deadline_failure: RuntimeProcessRecoveryReadinessTransitionFailureV2,
    refresh: Refresh,
    discord_terminal: DiscordTerminal,
    owner_terminal: OwnerTerminal,
    shutdown: Shutdown,
) -> Result<(), RuntimeProcessRecoveryReadinessTransitionFailureV2>
where
    Refresh: Future<Output = Result<(), RuntimeClosedRecoveryReadinessRefreshErrorV2>>,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
    OwnerTerminal: Future<Output = ()>,
    Shutdown: Future<Output = crate::RuntimeShutdownObservationV1>,
{
    if Instant::now() >= readiness_cutoff {
        return Err(deadline_failure);
    }
    tokio::pin!(refresh);
    tokio::pin!(discord_terminal);
    tokio::pin!(owner_terminal);
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        observation = &mut shutdown => {
            Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(
                    observation.cause(),
                ),
            ))
        }
        _ = sleep_until(TokioInstant::from_std(readiness_cutoff)) => {
            Err(deadline_failure)
        }
        transition = &mut discord_terminal => {
            Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(transition))
        }
        () = &mut owner_terminal => {
            Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ))
        }
        result = &mut refresh => {
            match result {
                Ok(()) => Ok(()),
                Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed) => {
                    Err(deadline_failure)
                }
                Err(error) => {
                    Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness(error.into()))
                }
            }
        }
    }
}

async fn shutdown_ready_recovery_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
    iteration: RuntimeClosedRecoveryReadyIterationV2,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    shutdown_paused_foundation_owner_v1(foundation, discord, move |deadline| {
        iteration.abort_and_shutdown_until_v2(deadline)
    })
    .await
}

fn finish_readiness_transition_v2(
    transition: RuntimeProcessRecoveryReadinessTransitionFailureV2,
    cleanup: Result<(), RuntimePausedConnectedProcessShutdownErrorV1>,
) -> RuntimeProcessRecoveryReadinessTransitionErrorV2 {
    match cleanup {
        Ok(()) => RuntimeProcessRecoveryReadinessTransitionErrorV2::Transition(transition),
        Err(cleanup) => RuntimeProcessRecoveryReadinessTransitionErrorV2::CleanupAfterTransition {
            transition,
            cleanup: cleanup.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, poll_fn, ready};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use super::*;
    use crate::{
        DatabaseCapabilityV1, RuntimeDatabasePoolShutdownErrorV1,
        RuntimeDiscordGatewayShutdownFailureV1, RuntimeOwnerHeldProcessShutdownErrorV1,
    };

    struct TrackedPendingReadinessV2 {
        polled: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    }

    impl Future for TrackedPendingReadinessV2 {
        type Output = Result<(), RuntimeClosedRecoveryReadinessRefreshErrorV2>;

        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            self.polled.store(true, Ordering::Release);
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }

    impl Drop for TrackedPendingReadinessV2 {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[test]
    fn public_failures_are_finite_contextual_and_redacted() {
        let failures = [
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
            RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness(
                RuntimeProcessRecoveryReadinessFailureV2::Database(
                    RuntimeDatabaseCompositionErrorV1::ReadinessRejected {
                        capability: DatabaseCapabilityV1::Panel,
                    },
                ),
            ),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness(
                RuntimeProcessRecoveryReadinessFailureV2::GatewayProtocolViolation,
            ),
        ];

        for failure in failures {
            assert!(!failure.code().is_empty());
            let error = RuntimeProcessRecoveryReadinessTransitionErrorV2::Transition(failure);
            assert!(!error.code().is_empty());
            assert_eq!(
                format!("{error:?}"),
                "RuntimeProcessRecoveryReadinessTransitionErrorV2(<redacted>)"
            );
            assert!(std::error::Error::source(&error).is_none());
        }
        assert_eq!(failures[2].context(), Some("panel"));
    }

    #[test]
    fn compound_cleanup_failure_is_preserved() {
        let cleanup = RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
            discord: RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed,
            owner_held: RuntimeOwnerHeldProcessShutdownErrorV1::Database(
                RuntimeDatabasePoolShutdownErrorV1::TimedOut,
            ),
        };
        let transition =
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed;
        let error = finish_readiness_transition_v2(transition, Err(cleanup));

        assert_eq!(
            error.code(),
            "runtime_process_recovery_readiness_transition_cleanup"
        );
        assert!(matches!(
            error,
            RuntimeProcessRecoveryReadinessTransitionErrorV2::CleanupAfterTransition {
                cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2::DiscordAndOwnerHeld { .. },
                ..
            }
        ));
    }

    #[test]
    fn readiness_deadline_preserves_the_expiring_authority() {
        let now = Instant::now();
        let operation = now + Duration::from_secs(2);
        let owner = now + Duration::from_secs(1);

        assert_eq!(
            classify_readiness_deadline_v2(operation, owner),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            )
        );
        assert_eq!(
            classify_readiness_deadline_v2(owner, operation),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed
        );
    }

    #[tokio::test]
    async fn readiness_race_prefers_discord_then_owner() {
        let cutoff = Instant::now() + Duration::from_secs(1);
        let deadline = RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed;
        let discord = await_recovery_readiness_refresh_v2(
            cutoff,
            deadline,
            ready(Ok(())),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
            ready(()),
            pending(),
        )
        .await;
        let owner = await_recovery_readiness_refresh_v2(
            cutoff,
            deadline,
            ready(Ok(())),
            pending(),
            ready(()),
            pending(),
        )
        .await;

        assert_eq!(
            discord,
            Err(
                RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                )
            )
        );
        assert_eq!(
            owner,
            Err(
                RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                )
            )
        );
    }

    #[tokio::test]
    async fn readiness_race_returns_success_with_live_terminals() {
        let result = await_recovery_readiness_refresh_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
            ready(Ok(())),
            pending(),
            pending(),
            pending(),
        )
        .await;

        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    async fn discord_terminal_cancels_an_already_polled_readiness_refresh() {
        let polled = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let refresh = TrackedPendingReadinessV2 {
            polled: polled.clone(),
            dropped: dropped.clone(),
        };
        let discord_polled = polled.clone();

        let result = await_recovery_readiness_refresh_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
            refresh,
            poll_fn(move |context| {
                if discord_polled.load(Ordering::Acquire) {
                    Poll::Ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated)
                } else {
                    context.waker().wake_by_ref();
                    Poll::Pending
                }
            }),
            pending(),
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(
                RuntimeProcessRecoveryReadinessTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                )
            )
        );
        assert!(polled.load(Ordering::Acquire));
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn readiness_race_maps_the_exact_failure() {
        let result = await_recovery_readiness_refresh_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
            ready(Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::Database(
                RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
            ))),
            pending(),
            pending(),
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(
                RuntimeProcessRecoveryReadinessTransitionFailureV2::Readiness(
                    RuntimeProcessRecoveryReadinessFailureV2::Database(
                        RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
                    ),
                )
            )
        );
    }

    #[tokio::test]
    async fn elapsed_readiness_cutoff_polls_no_authority_future() {
        let result = await_recovery_readiness_refresh_v2(
            Instant::now(),
            RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
            poll_fn(
                |_| -> Poll<Result<(), RuntimeClosedRecoveryReadinessRefreshErrorV2>> {
                    panic!("elapsed refresh must not be polled")
                },
            ),
            poll_fn(
                |_| -> Poll<RuntimeProcessPausedConnectedTransitionFailureV1> {
                    panic!("elapsed Discord terminal must not be polled")
                },
            ),
            poll_fn(|_| -> Poll<()> { panic!("elapsed owner terminal must not be polled") }),
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed)
        );
    }
}
