use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::closed_recovery::{
    begin_initial_empty_recovery_retained_v2, RuntimeClosedRecoveryBeginErrorV2,
    RuntimeClosedRecoveryPendingPhaseV2,
};
use crate::discord::RuntimeDiscordGatewaySupervisorV1;
use crate::gateway_owner_startup_watchdog::{
    RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2, RuntimeGatewayOwnerPreparedClosedRecoveryV2,
};
use crate::recovery_identity::generate_runtime_recovery_id_v2;
use crate::{
    RuntimeDatabaseCompositionErrorV1, RuntimePausedConnectedProcessShutdownErrorV1,
    RuntimeRecoveryIdGenerationErrorV2,
};

use super::connected::{
    discord_transition_failure_v1, finish_paused_connected_shutdown_v1,
    map_discord_shutdown_failure_v1, map_discord_transition_exit_v1,
    shutdown_paused_discord_owner_v1, RuntimePausedConnectedProcessV1,
    RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::owner::finish_runtime_owner_held_process_shutdown_v1;
use super::{RuntimeOwnerHeldProcessV1, RuntimeProcessFoundationV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessGatewayOwnerPrepareFailureV2 {
    SafetyElapsed,
    OwnershipLost,
    ObservationUnavailable,
    ProtocolViolation,
    SupervisorUnavailable,
}

impl From<RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2>
    for RuntimeProcessGatewayOwnerPrepareFailureV2
{
    fn from(error: RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2) -> Self {
        match error {
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed => Self::SafetyElapsed,
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost => Self::OwnershipLost,
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ObservationUnavailable => {
                Self::ObservationUnavailable
            }
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation => {
                Self::ProtocolViolation
            }
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SupervisorUnavailable => {
                Self::SupervisorUnavailable
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessClosedRecoveryBeginFailureV2 {
    DeadlineElapsed,
    GatewayObservation(crate::RuntimeGatewayReadyObservationErrorV1),
    GatewayCoordinator,
    GatewayProtocolViolation,
    Registry(crate::RuntimeRegistryRecoveryObservationErrorV1),
}

impl From<RuntimeClosedRecoveryBeginErrorV2> for RuntimeProcessClosedRecoveryBeginFailureV2 {
    fn from(error: RuntimeClosedRecoveryBeginErrorV2) -> Self {
        match error {
            RuntimeClosedRecoveryBeginErrorV2::DeadlineElapsed => Self::DeadlineElapsed,
            RuntimeClosedRecoveryBeginErrorV2::Gateway(error) => match error {
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
            RuntimeClosedRecoveryBeginErrorV2::Registry(error) => Self::Registry(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessRecoveryPendingTransitionFailureV2 {
    OperationDeadlineElapsed,
    PausedConnection(RuntimeProcessPausedConnectedTransitionFailureV1),
    RecoveryId(RuntimeRecoveryIdGenerationErrorV2),
    GatewayOwnerPrepare(RuntimeProcessGatewayOwnerPrepareFailureV2),
    DatabaseReadiness(RuntimeDatabaseCompositionErrorV1),
    ClosedRecovery(RuntimeProcessClosedRecoveryBeginFailureV2),
}

impl RuntimeProcessRecoveryPendingTransitionFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_process_recovery_pending_operation_deadline_elapsed"
            }
            Self::PausedConnection(error) => error.code(),
            Self::RecoveryId(error) => error.code(),
            Self::GatewayOwnerPrepare(error) => match error {
                RuntimeProcessGatewayOwnerPrepareFailureV2::SafetyElapsed => {
                    "runtime_gateway_owner_closed_recovery_prepare_safety_elapsed"
                }
                RuntimeProcessGatewayOwnerPrepareFailureV2::OwnershipLost => {
                    "runtime_gateway_owner_closed_recovery_prepare_ownership_lost"
                }
                RuntimeProcessGatewayOwnerPrepareFailureV2::ObservationUnavailable => {
                    "runtime_gateway_owner_closed_recovery_prepare_observation_unavailable"
                }
                RuntimeProcessGatewayOwnerPrepareFailureV2::ProtocolViolation => {
                    "runtime_gateway_owner_closed_recovery_prepare_protocol_violation"
                }
                RuntimeProcessGatewayOwnerPrepareFailureV2::SupervisorUnavailable => {
                    "runtime_gateway_owner_closed_recovery_prepare_supervisor_unavailable"
                }
            },
            Self::DatabaseReadiness(error) => error.code(),
            Self::ClosedRecovery(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::RecoveryId(error) => error.context(),
            Self::DatabaseReadiness(error) => error.context(),
            Self::OperationDeadlineElapsed
            | Self::PausedConnection(_)
            | Self::GatewayOwnerPrepare(_)
            | Self::ClosedRecovery(_) => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRecoveryPendingProcessCleanupFailureV2 {
    #[error("runtime recovery-pending process Discord shutdown failed")]
    Discord(crate::RuntimeDiscordGatewayShutdownFailureV1),
    #[error("runtime recovery-pending process owner-held shutdown failed")]
    OwnerHeld(crate::RuntimeOwnerHeldProcessShutdownErrorV1),
    #[error("runtime recovery-pending process Discord and owner-held shutdown failed")]
    DiscordAndOwnerHeld {
        discord: crate::RuntimeDiscordGatewayShutdownFailureV1,
        owner_held: crate::RuntimeOwnerHeldProcessShutdownErrorV1,
    },
}

impl RuntimeRecoveryPendingProcessCleanupFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Discord(error) => error.code(),
            Self::OwnerHeld(error) => error.code(),
            Self::DiscordAndOwnerHeld { .. } => {
                "runtime_recovery_pending_process_discord_and_owner_held_shutdown"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl From<RuntimePausedConnectedProcessShutdownErrorV1>
    for RuntimeRecoveryPendingProcessCleanupFailureV2
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

impl Debug for RuntimeRecoveryPendingProcessCleanupFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryPendingProcessCleanupFailureV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessRecoveryPendingTransitionErrorV2 {
    #[error("runtime process recovery-pending transition failed")]
    Transition(RuntimeProcessRecoveryPendingTransitionFailureV2),
    #[error("runtime process recovery-pending transition cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessRecoveryPendingTransitionFailureV2,
        cleanup: RuntimeRecoveryPendingProcessCleanupFailureV2,
    },
}

impl RuntimeProcessRecoveryPendingTransitionErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_recovery_pending_transition_cleanup"
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

impl Debug for RuntimeProcessRecoveryPendingTransitionErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessRecoveryPendingTransitionErrorV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRecoveryPendingProcessShutdownErrorV2 {
    #[error("runtime recovery-pending process shutdown failed")]
    Cleanup(RuntimeRecoveryPendingProcessCleanupFailureV2),
}

impl RuntimeRecoveryPendingProcessShutdownErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cleanup(error) => error.code(),
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeRecoveryPendingProcessShutdownErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryPendingProcessShutdownErrorV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRecoveryPendingProcessV2 {
    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,
    pub(super) foundation: RuntimeProcessFoundationV1,
    pub(super) pending: RuntimeClosedRecoveryPendingPhaseV2,
}

impl RuntimePausedConnectedProcessV1 {
    pub(crate) async fn into_recovery_pending_v2(
        self,
    ) -> Result<RuntimeRecoveryPendingProcessV2, RuntimeProcessRecoveryPendingTransitionErrorV2>
    {
        if let Err(error) = self.require_current_paused_connection_v1() {
            return Err(cleanup_paused_transition_v2(
                self,
                RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(error),
            )
            .await);
        }
        let recovery_id = match generate_runtime_recovery_id_v2() {
            Ok(recovery_id) => recovery_id,
            Err(error) => {
                return Err(cleanup_paused_transition_v2(
                    self,
                    RuntimeProcessRecoveryPendingTransitionFailureV2::RecoveryId(error),
                )
                .await);
            }
        };
        let RuntimePausedConnectedProcessV1 {
            mut discord,
            owner_held,
            paused_gateway,
        } = self;
        let RuntimeOwnerHeldProcessV1 {
            foundation,
            mut owner,
        } = owner_held;
        let operation_cutoff = foundation.startup_budget.operation_cutoff();
        let prepare = {
            let preparation = owner.prepare_closed_recovery_in_place_v2();
            let discord_terminal =
                async { map_discord_transition_exit_v1(discord.wait_terminal().await.exit()) };
            await_recovery_prepare_v2(operation_cutoff, preparation, discord_terminal).await
        };
        match prepare {
            Ok(()) => {}
            Err(transition) => {
                drop(paused_gateway);
                let owner_held = RuntimeOwnerHeldProcessV1 { foundation, owner };
                let cleanup = shutdown_paused_discord_owner_v1(owner_held, discord).await;
                return Err(finish_transition_v2(transition, cleanup));
            }
        }
        let mut prepared = match owner.try_into_prepared_closed_recovery_v2() {
            Ok(prepared) => prepared,
            Err(owner) => {
                let owner = *owner;
                drop(paused_gateway);
                let transition =
                    RuntimeProcessRecoveryPendingTransitionFailureV2::GatewayOwnerPrepare(
                        RuntimeProcessGatewayOwnerPrepareFailureV2::ProtocolViolation,
                    );
                let owner_held = RuntimeOwnerHeldProcessV1 { foundation, owner };
                let cleanup = shutdown_paused_discord_owner_v1(owner_held, discord).await;
                return Err(finish_transition_v2(transition, cleanup));
            }
        };
        let owner_safety_deadline = prepared.observation().safety_deadline();
        let verification_cutoff = operation_cutoff.min(owner_safety_deadline);
        let verification_deadline_failure =
            classify_verification_deadline_v2(operation_cutoff, owner_safety_deadline);
        let verification = foundation.databases.verify_readiness_v1();
        let discord_terminal =
            async { map_discord_transition_exit_v1(discord.wait_terminal().await.exit()) };
        let owner_terminal = async {
            prepared.wait_terminal().await;
        };
        let readiness = await_recovery_readiness_v2(
            verification_cutoff,
            verification_deadline_failure,
            verification,
            discord_terminal,
            owner_terminal,
        )
        .await;
        let readiness = match readiness {
            Ok(readiness) => readiness,
            Err(transition) => {
                drop(paused_gateway);
                let cleanup = shutdown_prepared_recovery_v2(foundation, discord, prepared).await;
                return Err(finish_transition_v2(transition, cleanup));
            }
        };
        if let Err(transition) =
            require_prepared_paused_connection_v2(&foundation, &discord, &prepared, &paused_gateway)
        {
            drop(paused_gateway);
            let cleanup = shutdown_prepared_recovery_v2(foundation, discord, prepared).await;
            return Err(finish_transition_v2(transition, cleanup));
        }
        let pending = begin_initial_empty_recovery_retained_v2(
            &foundation.gateway,
            &foundation.registry,
            prepared,
            recovery_id,
            &readiness,
            &paused_gateway,
            operation_cutoff,
        );
        drop(paused_gateway);
        let pending = match pending {
            Ok(pending) => pending,
            Err(failure) => {
                let (prepared, error) = failure.into_parts();
                let transition =
                    RuntimeProcessRecoveryPendingTransitionFailureV2::ClosedRecovery(error.into());
                let cleanup = shutdown_prepared_recovery_v2(foundation, discord, prepared).await;
                return Err(finish_transition_v2(transition, cleanup));
            }
        };
        let final_transition = if let Some(error) = discord_transition_failure_v1(&discord) {
            Some(RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(error))
        } else if !foundation.startup_budget.operation_is_open() {
            Some(RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed)
        } else {
            pending
                .revalidate_v2()
                .err()
                .map(RuntimeProcessClosedRecoveryBeginFailureV2::from)
                .map(RuntimeProcessRecoveryPendingTransitionFailureV2::ClosedRecovery)
        };
        if let Some(transition) = final_transition {
            let cleanup = shutdown_pending_recovery_v2(foundation, discord, pending).await;
            return Err(finish_transition_v2(transition, cleanup));
        }
        Ok(RuntimeRecoveryPendingProcessV2 {
            discord,
            foundation,
            pending,
        })
    }
}

impl Debug for RuntimeRecoveryPendingProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryPendingProcessV2(<redacted>)")
    }
}

fn require_prepared_paused_connection_v2(
    foundation: &RuntimeProcessFoundationV1,
    discord: &RuntimeDiscordGatewaySupervisorV1,
    prepared: &RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    paused_gateway: &automation_runtime_worker::RuntimePausedGatewayObservationV2,
) -> Result<(), RuntimeProcessRecoveryPendingTransitionFailureV2> {
    if !foundation.startup_budget.operation_is_open() {
        return Err(RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed);
    }
    if prepared.observation().safety_deadline() <= Instant::now() {
        return Err(
            RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        );
    }
    if prepared.terminal_status().is_some() {
        return Err(
            RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        );
    }
    if let Some(error) = discord_transition_failure_v1(discord) {
        return Err(RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(error));
    }
    match foundation.gateway.observe_paused_connected_gateway_v2() {
        Ok(current) if current == *paused_gateway => {}
        Ok(_) => {
            return Err(
                RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(
                        crate::RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
                    ),
                ),
            );
        }
        Err(error) => {
            return Err(
                RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(error),
                ),
            );
        }
    }
    if prepared.terminal_status().is_some() {
        return Err(
            RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        );
    }
    if let Some(error) = discord_transition_failure_v1(discord) {
        return Err(RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(error));
    }
    Ok(())
}

fn classify_verification_deadline_v2(
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessRecoveryPendingTransitionFailureV2 {
    if owner_safety_deadline <= operation_cutoff {
        RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    } else {
        RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed
    }
}

async fn await_recovery_prepare_v2<Preparation, DiscordTerminal>(
    operation_cutoff: Instant,
    preparation: Preparation,
    discord_terminal: DiscordTerminal,
) -> Result<(), RuntimeProcessRecoveryPendingTransitionFailureV2>
where
    Preparation: Future<Output = Result<(), RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2>>,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
{
    if Instant::now() >= operation_cutoff {
        return Err(RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed);
    }
    tokio::pin!(preparation);
    tokio::pin!(discord_terminal);
    tokio::select! {
        biased;
        _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => {
            Err(RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed)
        }
        transition = &mut discord_terminal => {
            Err(RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(transition))
        }
        result = &mut preparation => {
            result
                .map_err(RuntimeProcessGatewayOwnerPrepareFailureV2::from)
                .map_err(RuntimeProcessRecoveryPendingTransitionFailureV2::GatewayOwnerPrepare)
        }
    }
}

async fn await_recovery_readiness_v2<Verification, DiscordTerminal, OwnerTerminal>(
    verification_cutoff: Instant,
    deadline_failure: RuntimeProcessRecoveryPendingTransitionFailureV2,
    verification: Verification,
    discord_terminal: DiscordTerminal,
    owner_terminal: OwnerTerminal,
) -> Result<crate::RuntimeDatabaseReadinessV1, RuntimeProcessRecoveryPendingTransitionFailureV2>
where
    Verification: Future<
        Output = Result<crate::RuntimeDatabaseReadinessV1, RuntimeDatabaseCompositionErrorV1>,
    >,
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
    OwnerTerminal: Future<Output = ()>,
{
    if Instant::now() >= verification_cutoff {
        return Err(deadline_failure);
    }
    tokio::pin!(verification);
    tokio::pin!(discord_terminal);
    tokio::pin!(owner_terminal);
    tokio::select! {
        biased;
        _ = sleep_until(TokioInstant::from_std(verification_cutoff)) => {
            Err(deadline_failure)
        }
        transition = &mut discord_terminal => {
            Err(RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(transition))
        }
        () = &mut owner_terminal => {
            Err(RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ))
        }
        result = &mut verification => {
            result.map_err(RuntimeProcessRecoveryPendingTransitionFailureV2::DatabaseReadiness)
        }
    }
}

async fn cleanup_paused_transition_v2(
    paused: RuntimePausedConnectedProcessV1,
    transition: RuntimeProcessRecoveryPendingTransitionFailureV2,
) -> RuntimeProcessRecoveryPendingTransitionErrorV2 {
    finish_transition_v2(transition, paused.shutdown().await)
}

async fn shutdown_prepared_recovery_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
    prepared: RuntimeGatewayOwnerPreparedClosedRecoveryV2,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    let discord_shutdown = discord
        .shutdown_until(
            foundation.gateway.begin_discord_drain_v1(),
            foundation.startup_budget.discord_cleanup_deadline(),
        )
        .await
        .map_err(map_discord_shutdown_failure_v1);
    let owner = prepared
        .abort_and_shutdown_until_v2(foundation.startup_budget.owner_cleanup_deadline())
        .await;
    let database = foundation.shutdown().await;
    let owner_held = finish_runtime_owner_held_process_shutdown_v1(owner, database);
    finish_paused_connected_shutdown_v1(discord_shutdown, owner_held)
}

async fn shutdown_pending_recovery_v2(
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

fn finish_transition_v2(
    transition: RuntimeProcessRecoveryPendingTransitionFailureV2,
    cleanup: Result<(), RuntimePausedConnectedProcessShutdownErrorV1>,
) -> RuntimeProcessRecoveryPendingTransitionErrorV2 {
    match cleanup {
        Ok(()) => RuntimeProcessRecoveryPendingTransitionErrorV2::Transition(transition),
        Err(cleanup) => RuntimeProcessRecoveryPendingTransitionErrorV2::CleanupAfterTransition {
            transition,
            cleanup: cleanup.into(),
        },
    }
}

impl RuntimeProcessClosedRecoveryBeginFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::DeadlineElapsed => "runtime_closed_recovery_begin_deadline_elapsed",
            Self::GatewayObservation(error) => error.code(),
            Self::GatewayCoordinator => "runtime_gateway_recovery_coordinator_transition",
            Self::GatewayProtocolViolation => "runtime_gateway_recovery_protocol_violation",
            Self::Registry(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, poll_fn, ready};
    use std::task::Poll;
    use std::time::Duration;

    use super::*;
    use crate::database::runtime_database_readiness_for_test_v1;
    use crate::{
        DatabaseCapabilityV1, RuntimeDatabasePoolShutdownErrorV1,
        RuntimeDiscordGatewayShutdownFailureV1, RuntimeGatewayReadyObservationErrorV1,
        RuntimeOwnerHeldProcessShutdownErrorV1, RuntimeRegistryRecoveryObservationErrorV1,
    };

    #[test]
    fn transition_failures_have_finite_codes_context_and_redacted_diagnostics() {
        let failures = [
            RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
            RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
            RuntimeProcessRecoveryPendingTransitionFailureV2::RecoveryId(
                RuntimeRecoveryIdGenerationErrorV2::EntropyUnavailable,
            ),
            RuntimeProcessRecoveryPendingTransitionFailureV2::GatewayOwnerPrepare(
                RuntimeProcessGatewayOwnerPrepareFailureV2::OwnershipLost,
            ),
            RuntimeProcessRecoveryPendingTransitionFailureV2::DatabaseReadiness(
                RuntimeDatabaseCompositionErrorV1::ReadinessRejected {
                    capability: DatabaseCapabilityV1::Panel,
                },
            ),
            RuntimeProcessRecoveryPendingTransitionFailureV2::ClosedRecovery(
                RuntimeProcessClosedRecoveryBeginFailureV2::GatewayObservation(
                    RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused,
                ),
            ),
            RuntimeProcessRecoveryPendingTransitionFailureV2::ClosedRecovery(
                RuntimeProcessClosedRecoveryBeginFailureV2::Registry(
                    RuntimeRegistryRecoveryObservationErrorV1::NotEmpty,
                ),
            ),
        ];

        for failure in failures {
            assert!(!failure.code().is_empty());
            let error = RuntimeProcessRecoveryPendingTransitionErrorV2::Transition(failure);
            assert!(!error.code().is_empty());
            assert!(!error.to_string().is_empty());
            assert_eq!(
                format!("{error:?}"),
                "RuntimeProcessRecoveryPendingTransitionErrorV2(<redacted>)"
            );
            assert!(std::error::Error::source(&error).is_none());
        }
        assert_eq!(failures[4].context(), Some("panel"));
    }

    #[test]
    fn cleanup_failure_is_preserved_without_exposing_diagnostics() {
        let cleanup = RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
            discord: RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed,
            owner_held: RuntimeOwnerHeldProcessShutdownErrorV1::Database(
                RuntimeDatabasePoolShutdownErrorV1::TimedOut,
            ),
        };
        let transition = RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed;
        let error = finish_transition_v2(transition, Err(cleanup));

        assert_eq!(
            error.code(),
            "runtime_process_recovery_pending_transition_cleanup"
        );
        assert_eq!(error.context(), None);
        assert!(matches!(
            error,
            RuntimeProcessRecoveryPendingTransitionErrorV2::CleanupAfterTransition {
                transition:
                    RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
                cleanup: RuntimeRecoveryPendingProcessCleanupFailureV2::DiscordAndOwnerHeld { .. },
            }
        ));
    }

    #[test]
    fn shutdown_wrapper_uses_recovery_pending_cleanup_codes() {
        let error = RuntimeRecoveryPendingProcessShutdownErrorV2::Cleanup(
            RuntimeRecoveryPendingProcessCleanupFailureV2::Discord(
                RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed,
            ),
        );
        let compound = RuntimeRecoveryPendingProcessCleanupFailureV2::from(
            RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
                discord: RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed,
                owner_held: RuntimeOwnerHeldProcessShutdownErrorV1::Database(
                    RuntimeDatabasePoolShutdownErrorV1::TimedOut,
                ),
            },
        );

        assert_eq!(
            error.code(),
            "runtime_discord_gateway_shutdown_close_deadline_elapsed"
        );
        assert_eq!(
            compound.code(),
            "runtime_recovery_pending_process_discord_and_owner_held_shutdown"
        );
        assert_eq!(compound.context(), None);
        assert_eq!(error.context(), None);
        assert_eq!(
            format!("{compound:?}"),
            "RuntimeRecoveryPendingProcessCleanupFailureV2(<redacted>)"
        );
        assert_eq!(
            format!("{error:?}"),
            "RuntimeRecoveryPendingProcessShutdownErrorV2(<redacted>)"
        );
    }

    #[test]
    fn readiness_deadline_preserves_the_expiring_authority() {
        let now = Instant::now();
        let operation_cutoff = now + std::time::Duration::from_secs(2);
        let owner_safety_deadline = now + std::time::Duration::from_secs(1);

        assert_eq!(
            classify_verification_deadline_v2(operation_cutoff, owner_safety_deadline),
            RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            )
        );
        assert_eq!(
            classify_verification_deadline_v2(owner_safety_deadline, operation_cutoff),
            RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed
        );
        assert_eq!(
            classify_verification_deadline_v2(operation_cutoff, operation_cutoff),
            RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            )
        );
    }

    #[tokio::test]
    async fn prepare_race_prefers_a_terminal_discord_connection() {
        let result = await_recovery_prepare_v2(
            Instant::now() + Duration::from_secs(1),
            ready(Ok(())),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
        )
        .await;

        assert_eq!(
            result,
            Err(
                RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                ),
            )
        );
    }

    #[tokio::test]
    async fn prepare_race_maps_owner_prepare_failure() {
        let result = await_recovery_prepare_v2(
            Instant::now() + Duration::from_secs(1),
            ready(Err(
                RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost,
            )),
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(
                RuntimeProcessRecoveryPendingTransitionFailureV2::GatewayOwnerPrepare(
                    RuntimeProcessGatewayOwnerPrepareFailureV2::OwnershipLost,
                ),
            )
        );
    }

    #[tokio::test]
    async fn prepare_race_returns_success_with_a_live_discord_connection() {
        let result = await_recovery_prepare_v2(
            Instant::now() + Duration::from_secs(1),
            ready(Ok(())),
            pending(),
        )
        .await;

        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    async fn prepare_race_rejects_an_elapsed_operation_cutoff() {
        let result = await_recovery_prepare_v2(
            Instant::now(),
            poll_fn(
                |_| -> Poll<Result<(), RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2>> {
                    panic!("elapsed preparation future must not be polled")
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
            Err(RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed)
        );
    }

    #[tokio::test]
    async fn readiness_race_prefers_discord_then_owner_before_readiness() {
        let readiness = runtime_database_readiness_for_test_v1();
        let discord_result = await_recovery_readiness_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
            ready(Ok(readiness.clone())),
            ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
            ready(()),
        )
        .await;
        let owner_result = await_recovery_readiness_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
            ready(Ok(readiness)),
            pending(),
            ready(()),
        )
        .await;

        assert_eq!(
            discord_result,
            Err(
                RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                ),
            )
        );
        assert_eq!(
            owner_result,
            Err(
                RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                ),
            )
        );
    }

    #[tokio::test]
    async fn readiness_race_returns_verified_readiness_with_live_authorities() {
        let readiness = runtime_database_readiness_for_test_v1();
        let result = await_recovery_readiness_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
            ready(Ok(readiness.clone())),
            pending(),
            pending(),
        )
        .await;

        assert_eq!(result, Ok(readiness));
    }

    #[tokio::test]
    async fn readiness_race_maps_database_readiness_failure() {
        let failure = RuntimeDatabaseCompositionErrorV1::ReadinessRejected {
            capability: DatabaseCapabilityV1::Serving,
        };
        let result = await_recovery_readiness_v2(
            Instant::now() + Duration::from_secs(1),
            RuntimeProcessRecoveryPendingTransitionFailureV2::OperationDeadlineElapsed,
            ready(Err(failure)),
            pending(),
            pending(),
        )
        .await;

        assert_eq!(
            result,
            Err(RuntimeProcessRecoveryPendingTransitionFailureV2::DatabaseReadiness(failure),)
        );
    }

    #[tokio::test]
    async fn readiness_race_preserves_the_selected_elapsed_authority() {
        let failure = RuntimeProcessRecoveryPendingTransitionFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        );
        let result = await_recovery_readiness_v2(
            Instant::now(),
            failure,
            poll_fn(
                |_| -> Poll<
                    Result<crate::RuntimeDatabaseReadinessV1, RuntimeDatabaseCompositionErrorV1>,
                > { panic!("elapsed readiness future must not be polled") },
            ),
            poll_fn(
                |_| -> Poll<RuntimeProcessPausedConnectedTransitionFailureV1> {
                    panic!("elapsed Discord future must not be polled")
                },
            ),
            poll_fn(|_| -> Poll<()> { panic!("elapsed owner future must not be polled") }),
        )
        .await;

        assert_eq!(result, Err(failure));
    }
}
