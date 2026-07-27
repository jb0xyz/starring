use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::closed_recovery::{
    RuntimeClosedRecoveryCommitErrorV2, RuntimeClosedRecoveryPendingPhaseV2,
    RuntimeClosedRecoverySessionV2,
};
use crate::discord::RuntimeDiscordGatewaySupervisorV1;
use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerClosedRecoveryCommitErrorV2;
use crate::{
    RuntimePausedConnectedProcessShutdownErrorV1, RuntimeRegistryRecoveryObservationErrorV1,
};

use super::connected::{
    discord_transition_failure_v1, finish_paused_connected_shutdown_v1,
    map_discord_shutdown_failure_v1, map_discord_transition_exit_v1,
    RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::owner::finish_runtime_owner_held_process_shutdown_v1;
use super::recovery::RuntimeRecoveryPendingProcessV2;
use super::RuntimeProcessFoundationV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessGatewayOwnerCommitFailureV2 {
    SafetyElapsed,
    OwnerReceiptMismatch,
    ProtocolViolation,
    SupervisorUnavailable,
}

impl From<RuntimeGatewayOwnerClosedRecoveryCommitErrorV2>
    for RuntimeProcessGatewayOwnerCommitFailureV2
{
    fn from(error: RuntimeGatewayOwnerClosedRecoveryCommitErrorV2) -> Self {
        match error {
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed => Self::SafetyElapsed,
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::OwnerReceiptMismatch => {
                Self::OwnerReceiptMismatch
            }
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation => {
                Self::ProtocolViolation
            }
            RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SupervisorUnavailable => {
                Self::SupervisorUnavailable
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessClosedRecoveryCommitFailureV2 {
    DeadlineElapsed,
    GatewayObservation(crate::RuntimeGatewayReadyObservationErrorV1),
    GatewayCoordinator,
    GatewayProtocolViolation,
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    Owner(RuntimeProcessGatewayOwnerCommitFailureV2),
}

impl From<RuntimeClosedRecoveryCommitErrorV2> for RuntimeProcessClosedRecoveryCommitFailureV2 {
    fn from(error: RuntimeClosedRecoveryCommitErrorV2) -> Self {
        match error {
            RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed => Self::DeadlineElapsed,
            RuntimeClosedRecoveryCommitErrorV2::Gateway(error) => match error {
                crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(error) => {
                    Self::GatewayObservation(error)
                }
                crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Coordinator(_) => {
                    Self::GatewayCoordinator
                }
                crate::gateway::RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation => {
                    Self::GatewayProtocolViolation
                }
            },
            RuntimeClosedRecoveryCommitErrorV2::Registry(error) => Self::Registry(error),
            RuntimeClosedRecoveryCommitErrorV2::Owner(error) => Self::Owner(error.into()),
        }
    }
}

impl RuntimeProcessClosedRecoveryCommitFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::DeadlineElapsed => "runtime_closed_recovery_commit_deadline_elapsed",
            Self::GatewayObservation(error) => error.code(),
            Self::GatewayCoordinator => "runtime_closed_recovery_commit_gateway_coordinator",
            Self::GatewayProtocolViolation => {
                "runtime_closed_recovery_commit_gateway_protocol_violation"
            }
            Self::Registry(error) => error.code(),
            Self::Owner(error) => match error {
                RuntimeProcessGatewayOwnerCommitFailureV2::SafetyElapsed => {
                    "runtime_gateway_owner_closed_recovery_commit_safety_elapsed"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::OwnerReceiptMismatch => {
                    "runtime_gateway_owner_closed_recovery_commit_owner_receipt_mismatch"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::ProtocolViolation => {
                    "runtime_gateway_owner_closed_recovery_commit_protocol_violation"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::SupervisorUnavailable => {
                    "runtime_gateway_owner_closed_recovery_commit_supervisor_unavailable"
                }
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessClosedRecoveryTransitionFailureV2 {
    OperationDeadlineElapsed,
    PausedConnection(RuntimeProcessPausedConnectedTransitionFailureV1),
    Pending(crate::RuntimeProcessClosedRecoveryBeginFailureV2),
    Commit(RuntimeProcessClosedRecoveryCommitFailureV2),
}

impl RuntimeProcessClosedRecoveryTransitionFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_process_closed_recovery_operation_deadline_elapsed"
            }
            Self::PausedConnection(error) => error.code(),
            Self::Pending(error) => error.code(),
            Self::Commit(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeClosedRecoveryProcessCleanupFailureV2 {
    #[error("runtime closed-recovery process Discord shutdown failed")]
    Discord(crate::RuntimeDiscordGatewayShutdownFailureV1),
    #[error("runtime closed-recovery process owner-held shutdown failed")]
    OwnerHeld(crate::RuntimeOwnerHeldProcessShutdownErrorV1),
    #[error("runtime closed-recovery process Discord and owner-held shutdown failed")]
    DiscordAndOwnerHeld {
        discord: crate::RuntimeDiscordGatewayShutdownFailureV1,
        owner_held: crate::RuntimeOwnerHeldProcessShutdownErrorV1,
    },
}

impl RuntimeClosedRecoveryProcessCleanupFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Discord(error) => error.code(),
            Self::OwnerHeld(error) => error.code(),
            Self::DiscordAndOwnerHeld { .. } => {
                "runtime_closed_recovery_process_discord_and_owner_held_shutdown"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl From<RuntimePausedConnectedProcessShutdownErrorV1>
    for RuntimeClosedRecoveryProcessCleanupFailureV2
{
    fn from(error: RuntimePausedConnectedProcessShutdownErrorV1) -> Self {
        match error {
            RuntimePausedConnectedProcessShutdownErrorV1::Discord(error) => Self::Discord(error),
            RuntimePausedConnectedProcessShutdownErrorV1::OwnerHeld(error) => {
                Self::OwnerHeld(error)
            }
            RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
                discord,
                owner_held,
            } => Self::DiscordAndOwnerHeld {
                discord,
                owner_held,
            },
        }
    }
}

impl Debug for RuntimeClosedRecoveryProcessCleanupFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryProcessCleanupFailureV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessClosedRecoveryTransitionErrorV2 {
    #[error("runtime process closed-recovery transition failed")]
    Transition(RuntimeProcessClosedRecoveryTransitionFailureV2),
    #[error("runtime process closed-recovery transition cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessClosedRecoveryTransitionFailureV2,
        cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2,
    },
}

impl RuntimeProcessClosedRecoveryTransitionErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_closed_recovery_transition_cleanup"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeProcessClosedRecoveryTransitionErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessClosedRecoveryTransitionErrorV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeClosedRecoveryProcessShutdownErrorV2 {
    #[error("runtime closed-recovery process shutdown failed")]
    Cleanup(RuntimeClosedRecoveryProcessCleanupFailureV2),
}

impl RuntimeClosedRecoveryProcessShutdownErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cleanup(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeClosedRecoveryProcessShutdownErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryProcessShutdownErrorV2(<redacted>)")
    }
}

pub(crate) struct RuntimeClosedRecoveryProcessV2 {
    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,
    pub(super) foundation: RuntimeProcessFoundationV1,
    pub(super) session: RuntimeClosedRecoverySessionV2,
}

impl RuntimeRecoveryPendingProcessV2 {
    pub(crate) async fn into_closed_recovery_v2(
        self,
    ) -> Result<RuntimeClosedRecoveryProcessV2, RuntimeProcessClosedRecoveryTransitionErrorV2> {
        let RuntimeRecoveryPendingProcessV2 {
            mut discord,
            foundation,
            mut pending,
        } = self;
        if let Err(transition) =
            require_current_recovery_pending_v2(&foundation, &discord, &pending)
        {
            let cleanup = shutdown_pending_commit_v2(foundation, discord, pending).await;
            return Err(finish_closed_transition_v2(transition, cleanup));
        }
        let commit_cutoff = pending.commit_cutoff_v2();
        let commit = pending.commit_owner_in_place_v2();
        let discord_terminal =
            async { map_discord_transition_exit_v1(discord.wait_terminal().await.exit()) };
        let committed =
            await_closed_recovery_commit_v2(commit_cutoff, commit, discord_terminal).await;
        if let Err(transition) = committed {
            let cleanup = shutdown_pending_commit_v2(foundation, discord, pending).await;
            return Err(finish_closed_transition_v2(transition, cleanup));
        }
        let session = match pending.try_into_committed_session_v2() {
            Ok(session) => session,
            Err(pending) => {
                let transition = RuntimeProcessClosedRecoveryTransitionFailureV2::Commit(
                    RuntimeProcessClosedRecoveryCommitFailureV2::Owner(
                        RuntimeProcessGatewayOwnerCommitFailureV2::ProtocolViolation,
                    ),
                );
                let cleanup = shutdown_pending_commit_v2(foundation, discord, *pending).await;
                return Err(finish_closed_transition_v2(transition, cleanup));
            }
        };
        let session_validation = session
            .revalidate_v2()
            .map_err(RuntimeProcessClosedRecoveryCommitFailureV2::from)
            .map_err(RuntimeProcessClosedRecoveryTransitionFailureV2::Commit);
        let final_transition = if let Some(error) = discord_transition_failure_v1(&discord) {
            Some(RuntimeProcessClosedRecoveryTransitionFailureV2::PausedConnection(error))
        } else if !foundation.startup_budget.operation_is_open() {
            Some(RuntimeProcessClosedRecoveryTransitionFailureV2::OperationDeadlineElapsed)
        } else {
            session_validation.err()
        };
        if let Some(transition) = final_transition {
            let cleanup = shutdown_committed_recovery_v2(foundation, discord, session).await;
            return Err(finish_closed_transition_v2(transition, cleanup));
        }
        Ok(RuntimeClosedRecoveryProcessV2 {
            discord,
            foundation,
            session,
        })
    }
}

impl Debug for RuntimeClosedRecoveryProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryProcessV2(<redacted>)")
    }
}

fn require_current_recovery_pending_v2(
    foundation: &RuntimeProcessFoundationV1,
    discord: &RuntimeDiscordGatewaySupervisorV1,
    pending: &RuntimeClosedRecoveryPendingPhaseV2,
) -> Result<(), RuntimeProcessClosedRecoveryTransitionFailureV2> {
    if !foundation.startup_budget.operation_is_open() {
        return Err(RuntimeProcessClosedRecoveryTransitionFailureV2::OperationDeadlineElapsed);
    }
    if let Some(error) = discord_transition_failure_v1(discord) {
        return Err(RuntimeProcessClosedRecoveryTransitionFailureV2::PausedConnection(error));
    }
    pending
        .revalidate_v2()
        .map_err(crate::RuntimeProcessClosedRecoveryBeginFailureV2::from)
        .map_err(RuntimeProcessClosedRecoveryTransitionFailureV2::Pending)?;
    if let Some(error) = discord_transition_failure_v1(discord) {
        return Err(RuntimeProcessClosedRecoveryTransitionFailureV2::PausedConnection(error));
    }
    if !foundation.startup_budget.operation_is_open() {
        return Err(RuntimeProcessClosedRecoveryTransitionFailureV2::OperationDeadlineElapsed);
    }
    Ok(())
}

async fn await_closed_recovery_commit_v2<Commit, DiscordTerminal>(
    commit_cutoff: Instant,
    commit: Commit,
    discord_terminal: DiscordTerminal,
) -> Result<(), RuntimeProcessClosedRecoveryTransitionFailureV2>
where
    Commit: Future<Output = Result<(), RuntimeClosedRecoveryCommitErrorV2>>,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
{
    let deadline_failure = RuntimeProcessClosedRecoveryTransitionFailureV2::Commit(
        RuntimeProcessClosedRecoveryCommitFailureV2::DeadlineElapsed,
    );
    if Instant::now() >= commit_cutoff {
        return Err(deadline_failure);
    }
    tokio::pin!(commit);
    tokio::pin!(discord_terminal);
    tokio::select! {
        biased;
        _ = sleep_until(TokioInstant::from_std(commit_cutoff)) => Err(deadline_failure),
        transition = &mut discord_terminal => {
            Err(RuntimeProcessClosedRecoveryTransitionFailureV2::PausedConnection(transition))
        }
        result = &mut commit => {
            result
                .map_err(RuntimeProcessClosedRecoveryCommitFailureV2::from)
                .map_err(RuntimeProcessClosedRecoveryTransitionFailureV2::Commit)
        }
    }
}

async fn shutdown_pending_commit_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
    pending: RuntimeClosedRecoveryPendingPhaseV2,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    let discord_shutdown = discord
        .shutdown_until(
            foundation.gateway.begin_discord_drain_v1(),
            foundation.startup_budget.discord_cleanup_deadline(),
        )
        .await
        .map_err(map_discord_shutdown_failure_v1);
    let owner = pending
        .abort_and_shutdown_until_v2(foundation.startup_budget.owner_cleanup_deadline())
        .await;
    let database = foundation.shutdown().await;
    let owner_held = finish_runtime_owner_held_process_shutdown_v1(owner, database);
    finish_paused_connected_shutdown_v1(discord_shutdown, owner_held)
}

pub(super) async fn shutdown_committed_recovery_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
    session: RuntimeClosedRecoverySessionV2,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    let discord_shutdown = discord
        .shutdown_until(
            foundation.gateway.begin_discord_drain_v1(),
            foundation.startup_budget.discord_cleanup_deadline(),
        )
        .await
        .map_err(map_discord_shutdown_failure_v1);
    let owner = session
        .abort_and_shutdown_until_v2(foundation.startup_budget.owner_cleanup_deadline())
        .await;
    let database = foundation.shutdown().await;
    let owner_held = finish_runtime_owner_held_process_shutdown_v1(owner, database);
    finish_paused_connected_shutdown_v1(discord_shutdown, owner_held)
}

fn finish_closed_transition_v2(
    transition: RuntimeProcessClosedRecoveryTransitionFailureV2,
    cleanup: Result<(), RuntimePausedConnectedProcessShutdownErrorV1>,
) -> RuntimeProcessClosedRecoveryTransitionErrorV2 {
    match cleanup {
        Ok(()) => RuntimeProcessClosedRecoveryTransitionErrorV2::Transition(transition),
        Err(cleanup) => RuntimeProcessClosedRecoveryTransitionErrorV2::CleanupAfterTransition {
            transition,
            cleanup: cleanup.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, poll_fn, ready};
    use std::task::Poll;
    use std::time::Duration;

    use super::*;
    use crate::{
        RuntimeDatabasePoolShutdownErrorV1, RuntimeDiscordGatewayShutdownFailureV1,
        RuntimeOwnerHeldProcessShutdownErrorV1,
    };

    #[test]
    fn public_failures_have_finite_codes_and_redacted_diagnostics() {
        let failures = [
            RuntimeProcessClosedRecoveryTransitionFailureV2::OperationDeadlineElapsed,
            RuntimeProcessClosedRecoveryTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
            RuntimeProcessClosedRecoveryTransitionFailureV2::Commit(
                RuntimeProcessClosedRecoveryCommitFailureV2::DeadlineElapsed,
            ),
            RuntimeProcessClosedRecoveryTransitionFailureV2::Commit(
                RuntimeProcessClosedRecoveryCommitFailureV2::Owner(
                    RuntimeProcessGatewayOwnerCommitFailureV2::OwnerReceiptMismatch,
                ),
            ),
        ];

        for failure in failures {
            assert!(!failure.code().is_empty());
            let error = RuntimeProcessClosedRecoveryTransitionErrorV2::Transition(failure);
            assert!(!error.code().is_empty());
            assert_eq!(error.context(), None);
            assert_eq!(
                format!("{error:?}"),
                "RuntimeProcessClosedRecoveryTransitionErrorV2(<redacted>)"
            );
        }
    }

    #[test]
    fn compound_cleanup_failure_is_preserved_and_redacted() {
        let cleanup = RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
            discord: RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed,
            owner_held: RuntimeOwnerHeldProcessShutdownErrorV1::Database(
                RuntimeDatabasePoolShutdownErrorV1::TimedOut,
            ),
        };
        let transition = RuntimeProcessClosedRecoveryTransitionFailureV2::OperationDeadlineElapsed;
        let error = finish_closed_transition_v2(transition, Err(cleanup));

        assert_eq!(
            error.code(),
            "runtime_process_closed_recovery_transition_cleanup"
        );
        assert!(matches!(
            error,
            RuntimeProcessClosedRecoveryTransitionErrorV2::CleanupAfterTransition {
                cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2::DiscordAndOwnerHeld { .. },
                ..
            }
        ));
    }

    #[tokio::test]
    async fn commit_race_prefers_a_terminal_discord_connection() {
        let result = await_closed_recovery_commit_v2(
            Instant::now() + Duration::from_secs(1),
            ready(Ok(())),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
        )
        .await;

        assert_eq!(
            result,
            Err(
                RuntimeProcessClosedRecoveryTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                )
            )
        );
    }

    #[tokio::test]
    async fn commit_race_returns_success_with_a_live_discord_connection() {
        let result = await_closed_recovery_commit_v2(
            Instant::now() + Duration::from_secs(1),
            ready(Ok(())),
            pending(),
        )
        .await;

        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    async fn commit_race_maps_the_exact_commit_failure() {
        let result = await_closed_recovery_commit_v2(
            Instant::now() + Duration::from_secs(1),
            ready(Err(RuntimeClosedRecoveryCommitErrorV2::Owner(
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SupervisorUnavailable,
            ))),
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(RuntimeProcessClosedRecoveryTransitionFailureV2::Commit(
                RuntimeProcessClosedRecoveryCommitFailureV2::Owner(
                    RuntimeProcessGatewayOwnerCommitFailureV2::SupervisorUnavailable,
                ),
            ))
        );
    }

    #[tokio::test]
    async fn elapsed_commit_cutoff_polls_no_authority_future() {
        let result = await_closed_recovery_commit_v2(
            Instant::now(),
            poll_fn(
                |_| -> Poll<Result<(), RuntimeClosedRecoveryCommitErrorV2>> {
                    panic!("elapsed commit future must not be polled")
                },
            ),
            poll_fn(
                |_| -> Poll<RuntimeProcessPausedConnectedTransitionFailureV1> {
                    panic!("elapsed Discord future must not be polled")
                },
            ),
        )
        .await;

        assert_eq!(
            result,
            Err(RuntimeProcessClosedRecoveryTransitionFailureV2::Commit(
                RuntimeProcessClosedRecoveryCommitFailureV2::DeadlineElapsed,
            ))
        );
    }
}
