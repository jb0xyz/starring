use std::fmt::{Debug, Formatter};

use automation_runtime_worker::RuntimePausedGatewayObservationV2;
use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::discord::{
    RuntimeDiscordGatewayExitV1, RuntimeDiscordGatewayShutdownErrorV1,
    RuntimeDiscordGatewayStartErrorV1, RuntimeDiscordGatewaySupervisorV1,
};
use crate::gateway::RuntimeGatewayReadyObservationErrorV1;

use super::owner::RuntimeOwnerHeldProcessV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessPausedConnectedTransitionFailureV1 {
    OperationDeadlineElapsed,
    DiscordRuntimeUnavailable,
    DiscordAlreadyStarted,
    DiscordAdmissionOpened,
    DiscordTerminated,
    GatewayOwnerTerminated,
    GatewayObservationClosed,
    GatewayObservation(RuntimeGatewayReadyObservationErrorV1),
}

impl RuntimeProcessPausedConnectedTransitionFailureV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_process_paused_connected_operation_deadline_elapsed"
            }
            Self::DiscordRuntimeUnavailable => {
                "runtime_process_paused_connected_discord_runtime_unavailable"
            }
            Self::DiscordAlreadyStarted => {
                "runtime_process_paused_connected_discord_already_started"
            }
            Self::DiscordAdmissionOpened => {
                "runtime_process_paused_connected_discord_admission_opened"
            }
            Self::DiscordTerminated => "runtime_process_paused_connected_discord_terminated",
            Self::GatewayOwnerTerminated => {
                "runtime_process_paused_connected_gateway_owner_terminated"
            }
            Self::GatewayObservationClosed => {
                "runtime_process_paused_connected_gateway_observation_closed"
            }
            Self::GatewayObservation(error) => error.code(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDiscordGatewayShutdownFailureV1 {
    DeadlineElapsed,
    TaskStopped,
    CloseDeadlineElapsed,
    UnexpectedExit,
}

impl RuntimeDiscordGatewayShutdownFailureV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::DeadlineElapsed => "runtime_discord_gateway_shutdown_deadline_elapsed",
            Self::TaskStopped => "runtime_discord_gateway_shutdown_task_stopped",
            Self::CloseDeadlineElapsed => "runtime_discord_gateway_shutdown_close_deadline_elapsed",
            Self::UnexpectedExit => "runtime_discord_gateway_shutdown_unexpected_exit",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePausedConnectedProcessShutdownErrorV1 {
    #[error("runtime paused-connected process Discord shutdown failed")]
    Discord(RuntimeDiscordGatewayShutdownFailureV1),
    #[error("runtime paused-connected process owner-held shutdown failed")]
    OwnerHeld(super::RuntimeOwnerHeldProcessShutdownErrorV1),
    #[error("runtime paused-connected process Discord and owner-held shutdown failed")]
    DiscordAndOwnerHeld {
        discord: RuntimeDiscordGatewayShutdownFailureV1,
        owner_held: super::RuntimeOwnerHeldProcessShutdownErrorV1,
    },
}

impl RuntimePausedConnectedProcessShutdownErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Discord(error) => error.code(),
            Self::OwnerHeld(error) => error.code(),
            Self::DiscordAndOwnerHeld { .. } => {
                "runtime_paused_connected_process_discord_and_owner_held_shutdown"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimePausedConnectedProcessShutdownErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePausedConnectedProcessShutdownErrorV1(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessPausedConnectedTransitionErrorV1 {
    #[error("runtime process paused-connected transition failed")]
    Transition(RuntimeProcessPausedConnectedTransitionFailureV1),
    #[error("runtime process paused-connected transition cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessPausedConnectedTransitionFailureV1,
        cleanup: RuntimePausedConnectedProcessShutdownErrorV1,
    },
}

impl RuntimeProcessPausedConnectedTransitionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_paused_connected_transition_cleanup"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

impl Debug for RuntimeProcessPausedConnectedTransitionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessPausedConnectedTransitionErrorV1(<redacted>)")
    }
}

pub(crate) struct RuntimeDiscordStartingProcessV1 {
    discord: RuntimeDiscordGatewaySupervisorV1,
    owner_held: RuntimeOwnerHeldProcessV1,
}

pub(crate) struct RuntimeDiscordStartingProcessFailureV1 {
    owner_held: Box<RuntimeOwnerHeldProcessV1>,
    transition: RuntimeProcessPausedConnectedTransitionFailureV1,
}

pub(crate) struct RuntimeDiscordStartingProcessTransitionFailureV1 {
    starting: Box<RuntimeDiscordStartingProcessV1>,
    transition: RuntimeProcessPausedConnectedTransitionFailureV1,
}

pub(crate) struct RuntimePausedConnectedProcessV1 {
    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,
    pub(super) owner_held: RuntimeOwnerHeldProcessV1,
    pub(super) paused_gateway: RuntimePausedGatewayObservationV2,
}

impl RuntimeOwnerHeldProcessV1 {
    pub(crate) async fn begin_paused_discord_connection_v1(
        mut self,
    ) -> Result<RuntimeDiscordStartingProcessV1, RuntimeDiscordStartingProcessFailureV1> {
        if !self.foundation.startup_budget.operation_is_open() {
            return Err(RuntimeDiscordStartingProcessFailureV1 {
                owner_held: Box::new(self),
                transition:
                    RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed,
            });
        }
        let operation_cutoff = self.foundation.startup_budget.operation_cutoff();
        let discord_cleanup_deadline = self.foundation.startup_budget.discord_cleanup_deadline();
        let discord = self
            .foundation
            .gateway
            .start_discord_gateway_v1(
                self.foundation.secrets.discord_bot_token(),
                operation_cutoff,
                discord_cleanup_deadline,
            )
            .await;
        let discord = match discord {
            Ok(discord) => discord,
            Err(error) => {
                let transition = match error {
                    RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable => {
                        RuntimeProcessPausedConnectedTransitionFailureV1::DiscordRuntimeUnavailable
                    }
                    RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable => {
                        RuntimeProcessPausedConnectedTransitionFailureV1::DiscordAlreadyStarted
                    }
                    RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated => {
                        RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated
                    }
                    RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed => {
                        RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed
                    }
                };
                return Err(RuntimeDiscordStartingProcessFailureV1 {
                    owner_held: Box::new(self),
                    transition,
                });
            }
        };
        Ok(RuntimeDiscordStartingProcessV1 {
            discord,
            owner_held: self,
        })
    }
}

impl RuntimeDiscordStartingProcessFailureV1 {
    pub(crate) async fn cleanup(self) -> RuntimeProcessPausedConnectedTransitionErrorV1 {
        match (*self.owner_held).shutdown().await {
            Ok(()) => RuntimeProcessPausedConnectedTransitionErrorV1::Transition(self.transition),
            Err(owner_held) => {
                RuntimeProcessPausedConnectedTransitionErrorV1::CleanupAfterTransition {
                    transition: self.transition,
                    cleanup: RuntimePausedConnectedProcessShutdownErrorV1::OwnerHeld(owner_held),
                }
            }
        }
    }
}

impl RuntimeDiscordStartingProcessV1 {
    pub(crate) async fn wait_for_paused_connected_v1(
        &mut self,
    ) -> Result<RuntimePausedGatewayObservationV2, RuntimeProcessPausedConnectedTransitionFailureV1>
    {
        let operation_cutoff = self.owner_held.foundation.startup_budget.operation_cutoff();
        let mut changes = self
            .owner_held
            .foundation
            .gateway
            .admission_change_watch_v1();
        loop {
            self.require_live_paused_connection_v1()?;
            match self
                .owner_held
                .foundation
                .gateway
                .observe_paused_connected_gateway_v2()
            {
                Ok(first) => {
                    self.require_live_paused_connection_v1()?;
                    match self
                        .owner_held
                        .foundation
                        .gateway
                        .observe_paused_connected_gateway_v2()
                    {
                        Ok(current) if current == first => {
                            self.require_live_paused_connection_v1()?;
                            return Ok(current);
                        }
                        Ok(_) => continue,
                        Err(error) if retryable_gateway_observation_v1(error) => continue,
                        Err(error) => {
                            return Err(
                                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(
                                    error,
                                ),
                            );
                        }
                    }
                }
                Err(error) if retryable_gateway_observation_v1(error) => {}
                Err(error) => {
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(error),
                    );
                }
            }
            tokio::select! {
                biased;
                _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => {
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed,
                    );
                }
                _owner = self.owner_held.owner.wait_terminal() => {
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                    );
                }
                discord = self.discord.wait_terminal() => {
                    return Err(map_discord_transition_exit_v1(discord.exit()));
                }
                changed = changes.changed() => {
                    if !changed {
                        return Err(
                            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservationClosed,
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn into_paused_connected_v1(
        self,
        paused_gateway: RuntimePausedGatewayObservationV2,
    ) -> Result<RuntimePausedConnectedProcessV1, RuntimeDiscordStartingProcessTransitionFailureV1>
    {
        if let Err(transition) = self.require_live_paused_connection_v1() {
            return Err(RuntimeDiscordStartingProcessTransitionFailureV1 {
                starting: Box::new(self),
                transition,
            });
        }
        let current = self
            .owner_held
            .foundation
            .gateway
            .observe_paused_connected_gateway_v2();
        if current.as_ref() != Ok(&paused_gateway) {
            let transition = match current {
                Ok(_) => RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(
                    RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
                ),
                Err(error) => {
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(error)
                }
            };
            return Err(RuntimeDiscordStartingProcessTransitionFailureV1 {
                starting: Box::new(self),
                transition,
            });
        }
        if let Err(transition) = self.require_live_paused_connection_v1() {
            return Err(RuntimeDiscordStartingProcessTransitionFailureV1 {
                starting: Box::new(self),
                transition,
            });
        }
        Ok(RuntimePausedConnectedProcessV1 {
            discord: self.discord,
            owner_held: self.owner_held,
            paused_gateway,
        })
    }

    pub(crate) async fn cleanup_after_transition_failure_v1(
        self,
        transition: RuntimeProcessPausedConnectedTransitionFailureV1,
    ) -> RuntimeProcessPausedConnectedTransitionErrorV1 {
        let cleanup = shutdown_paused_discord_owner_v1(self.owner_held, self.discord).await;
        match cleanup {
            Ok(()) => RuntimeProcessPausedConnectedTransitionErrorV1::Transition(transition),
            Err(cleanup) => {
                RuntimeProcessPausedConnectedTransitionErrorV1::CleanupAfterTransition {
                    transition,
                    cleanup,
                }
            }
        }
    }

    fn require_live_paused_connection_v1(
        &self,
    ) -> Result<(), RuntimeProcessPausedConnectedTransitionFailureV1> {
        if !self
            .owner_held
            .foundation
            .startup_budget
            .operation_is_open()
        {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed);
        }
        if self.owner_held.owner.terminal_status().is_some() {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated);
        }
        if let Some(transition) = discord_transition_failure_v1(&self.discord) {
            return Err(transition);
        }
        Ok(())
    }
}

impl RuntimeDiscordStartingProcessTransitionFailureV1 {
    pub(crate) async fn cleanup(self) -> RuntimeProcessPausedConnectedTransitionErrorV1 {
        (*self.starting)
            .cleanup_after_transition_failure_v1(self.transition)
            .await
    }
}

impl RuntimePausedConnectedProcessV1 {
    pub(super) fn require_current_paused_connection_v1(
        &self,
    ) -> Result<(), RuntimeProcessPausedConnectedTransitionFailureV1> {
        if !self
            .owner_held
            .foundation
            .startup_budget
            .operation_is_open()
        {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed);
        }
        if self.owner_held.owner.terminal_status().is_some() {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated);
        }
        if let Some(transition) = discord_transition_failure_v1(&self.discord) {
            return Err(transition);
        }
        match self
            .owner_held
            .foundation
            .gateway
            .observe_paused_connected_gateway_v2()
        {
            Ok(current) if current == self.paused_gateway => {}
            Ok(_) => {
                return Err(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(
                        RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
                    ),
                );
            }
            Err(error) => {
                return Err(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(error),
                );
            }
        }
        if self.owner_held.owner.terminal_status().is_some() {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated);
        }
        if let Some(transition) = discord_transition_failure_v1(&self.discord) {
            return Err(transition);
        }
        Ok(())
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
        let Self {
            owner_held,
            discord,
            paused_gateway,
        } = self;
        drop(paused_gateway);
        shutdown_paused_discord_owner_v1(owner_held, discord).await
    }
}

impl Debug for RuntimeDiscordStartingProcessV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordStartingProcessV1(<redacted>)")
    }
}

impl Debug for RuntimePausedConnectedProcessV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePausedConnectedProcessV1(<redacted>)")
    }
}

fn retryable_gateway_observation_v1(error: RuntimeGatewayReadyObservationErrorV1) -> bool {
    matches!(
        error,
        RuntimeGatewayReadyObservationErrorV1::StaleConnectionEpoch
            | RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot
            | RuntimeGatewayReadyObservationErrorV1::NotConnected
            | RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent
    )
}

pub(super) fn discord_transition_failure_v1(
    discord: &RuntimeDiscordGatewaySupervisorV1,
) -> Option<RuntimeProcessPausedConnectedTransitionFailureV1> {
    discord
        .terminal_status()
        .map(|terminal| map_discord_transition_exit_v1(terminal.exit()))
        .or_else(|| {
            discord
                .is_finished()
                .then_some(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated)
        })
}

pub(super) fn map_discord_transition_exit_v1(
    exit: RuntimeDiscordGatewayExitV1,
) -> RuntimeProcessPausedConnectedTransitionFailureV1 {
    match exit {
        RuntimeDiscordGatewayExitV1::AdmissionOpened => {
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordAdmissionOpened
        }
        RuntimeDiscordGatewayExitV1::StartDeadlineElapsed => {
            RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed
        }
        RuntimeDiscordGatewayExitV1::Commanded
        | RuntimeDiscordGatewayExitV1::ControlOrphaned
        | RuntimeDiscordGatewayExitV1::StreamEnded
        | RuntimeDiscordGatewayExitV1::RuntimeFailure => {
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated
        }
    }
}

pub(super) async fn shutdown_paused_discord_owner_v1(
    owner_held: RuntimeOwnerHeldProcessV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    let discord_cleanup_deadline = owner_held
        .foundation
        .startup_budget
        .discord_cleanup_deadline();
    let discord_shutdown = discord
        .shutdown_until(
            owner_held.foundation.gateway.begin_discord_drain_v1(),
            discord_cleanup_deadline,
        )
        .await
        .map_err(map_discord_shutdown_failure_v1);
    let owner_shutdown = owner_held.shutdown().await;
    finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown)
}

pub(super) fn finish_paused_connected_shutdown_v1<T>(
    discord_shutdown: Result<T, RuntimeDiscordGatewayShutdownFailureV1>,
    owner_shutdown: Result<(), super::RuntimeOwnerHeldProcessShutdownErrorV1>,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    match (discord_shutdown, owner_shutdown) {
        (Ok(_), Ok(())) => Ok(()),
        (Err(discord), Ok(())) => Err(RuntimePausedConnectedProcessShutdownErrorV1::Discord(
            discord,
        )),
        (Ok(_), Err(owner_held)) => Err(RuntimePausedConnectedProcessShutdownErrorV1::OwnerHeld(
            owner_held,
        )),
        (Err(discord), Err(owner_held)) => Err(
            RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
                discord,
                owner_held,
            },
        ),
    }
}

pub(super) fn map_discord_shutdown_failure_v1(
    error: RuntimeDiscordGatewayShutdownErrorV1,
) -> RuntimeDiscordGatewayShutdownFailureV1 {
    match error {
        RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed => {
            RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed
        }
        RuntimeDiscordGatewayShutdownErrorV1::TaskStopped => {
            RuntimeDiscordGatewayShutdownFailureV1::TaskStopped
        }
        RuntimeDiscordGatewayShutdownErrorV1::CloseDeadlineElapsed => {
            RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed
        }
        RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal)
            if terminal.exit() == RuntimeDiscordGatewayExitV1::Commanded =>
        {
            RuntimeDiscordGatewayShutdownFailureV1::TaskStopped
        }
        RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(_) => {
            RuntimeDiscordGatewayShutdownFailureV1::UnexpectedExit
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{RuntimeDatabasePoolShutdownErrorV1, RuntimeOwnerHeldProcessShutdownErrorV1};

    use super::*;

    fn owner_failure() -> RuntimeOwnerHeldProcessShutdownErrorV1 {
        RuntimeOwnerHeldProcessShutdownErrorV1::Database(
            RuntimeDatabasePoolShutdownErrorV1::TimedOut,
        )
    }

    #[test]
    fn shutdown_classification_preserves_discord_and_owner_failures() {
        let commanded = Ok(RuntimeDiscordGatewayExitV1::Commanded);
        let discord: Result<RuntimeDiscordGatewayExitV1, _> =
            Err(RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed);

        assert_eq!(
            finish_paused_connected_shutdown_v1(commanded, Ok(())),
            Ok(())
        );
        assert_eq!(
            finish_paused_connected_shutdown_v1(discord, Ok(())),
            Err(RuntimePausedConnectedProcessShutdownErrorV1::Discord(
                RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed
            ))
        );
        assert_eq!(
            finish_paused_connected_shutdown_v1(commanded, Err(owner_failure())),
            Err(RuntimePausedConnectedProcessShutdownErrorV1::OwnerHeld(
                owner_failure()
            ))
        );
        assert_eq!(
            finish_paused_connected_shutdown_v1(discord, Err(owner_failure())),
            Err(
                RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
                    discord: RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed,
                    owner_held: owner_failure(),
                }
            )
        );
    }

    #[test]
    fn public_failures_have_finite_codes_and_redacted_diagnostics() {
        let transitions = [
            RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed,
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordRuntimeUnavailable,
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordAlreadyStarted,
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordAdmissionOpened,
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservationClosed,
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(
                RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused,
            ),
        ];
        for transition in transitions {
            assert!(!transition.code().is_empty());
            let error = RuntimeProcessPausedConnectedTransitionErrorV1::Transition(transition);
            assert!(!error.code().is_empty());
            assert_eq!(
                format!("{error:?}"),
                "RuntimeProcessPausedConnectedTransitionErrorV1(<redacted>)"
            );
            assert!(!error.to_string().is_empty());
            assert!(std::error::Error::source(&error).is_none());
        }
        let shutdown = RuntimePausedConnectedProcessShutdownErrorV1::Discord(
            RuntimeDiscordGatewayShutdownFailureV1::UnexpectedExit,
        );
        assert_eq!(
            format!("{shutdown:?}"),
            "RuntimePausedConnectedProcessShutdownErrorV1(<redacted>)"
        );
        assert!(!shutdown.code().is_empty());
        assert!(std::error::Error::source(&shutdown).is_none());
        assert_eq!(
            RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed.code(),
            "runtime_discord_gateway_shutdown_close_deadline_elapsed"
        );
        assert_eq!(
            map_discord_transition_exit_v1(RuntimeDiscordGatewayExitV1::AdmissionOpened),
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordAdmissionOpened
        );
    }

    #[test]
    fn only_transient_snapshot_races_are_retryable() {
        for retryable in [
            RuntimeGatewayReadyObservationErrorV1::StaleConnectionEpoch,
            RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
            RuntimeGatewayReadyObservationErrorV1::NotConnected,
            RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent,
        ] {
            assert!(retryable_gateway_observation_v1(retryable));
        }
        for terminal in [
            RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused,
            RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
            RuntimeGatewayReadyObservationErrorV1::Draining,
            RuntimeGatewayReadyObservationErrorV1::Stopped,
            RuntimeGatewayReadyObservationErrorV1::ControlOrphaned,
        ] {
            assert!(!retryable_gateway_observation_v1(terminal));
        }
    }
}
