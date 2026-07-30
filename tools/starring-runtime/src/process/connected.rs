use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use automation_runtime_worker::RuntimePausedGatewayObservationV2;
use tokio::time::{sleep_until, Instant as TokioInstant};

use crate::discord::{
    RuntimeDiscordGatewayExitV1, RuntimeDiscordGatewayShutdownErrorV1,
    RuntimeDiscordGatewayStartErrorV1, RuntimeDiscordGatewaySupervisorV1,
};
use crate::gateway::{RuntimeDiscordProductionStartV1, RuntimeGatewayReadyObservationErrorV1};
use crate::lifecycle_timing::{
    RuntimeLifecycleTimingMetricV2, RuntimeLifecycleTimingOutcomeV2,
    RuntimeLifecycleTimingTerminalReporterV2,
};
use crate::RuntimeShutdownCauseV1;

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
    ProcessShutdown(RuntimeShutdownCauseV1),
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
            Self::ProcessShutdown(cause) => cause.code(),
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
        let interaction_dispatch = self.foundation.interaction_dispatch_port_v1();
        let gateway_config = self.foundation.config.gateway();
        let product_readiness = self.foundation.product_readiness_observer_v1();
        let mut shutdown = self.foundation.shutdown_observer_v1();
        let discord = self
            .foundation
            .gateway
            .start_discord_gateway_v1(
                RuntimeDiscordProductionStartV1::new(
                    self.foundation.secrets.discord_bot_token(),
                    interaction_dispatch,
                    gateway_config,
                    product_readiness,
                    operation_cutoff,
                    discord_cleanup_deadline,
                ),
                &mut shutdown,
            )
            .await;
        let discord = match discord {
            Ok(discord) => discord,
            Err(error) => {
                let transition = match shutdown.observed() {
                    Some(observation) => {
                        RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(
                            observation.cause(),
                        )
                    }
                    None => match error {
                        RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable => {
                            RuntimeProcessPausedConnectedTransitionFailureV1::
                                DiscordRuntimeUnavailable
                        }
                        RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable => {
                            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordAlreadyStarted
                        }
                        RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated => {
                            self.foundation
                                .trip_shutdown_v1(RuntimeShutdownCauseV1::GatewayOwnerTerminal);
                            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated
                        }
                        RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed => {
                            RuntimeProcessPausedConnectedTransitionFailureV1::
                                OperationDeadlineElapsed
                        }
                    },
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
        let mut shutdown = self.owner_held.foundation.shutdown_observer_v1();
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
                            self.owner_held
                                .foundation
                                .trip_shutdown_v1(gateway_observation_shutdown_cause_v1(error));
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
                    self.owner_held
                        .foundation
                        .trip_shutdown_v1(gateway_observation_shutdown_cause_v1(error));
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(error),
                    );
                }
            }
            tokio::select! {
                biased;
                observation = shutdown.wait() => {
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(
                            observation.cause(),
                        ),
                    );
                }
                _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => {
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed,
                    );
                }
                _owner = self.owner_held.owner.wait_terminal() => {
                    self.owner_held
                        .foundation
                        .trip_shutdown_v1(RuntimeShutdownCauseV1::GatewayOwnerTerminal);
                    return Err(
                        RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                    );
                }
                discord = self.discord.wait_terminal() => {
                    self.owner_held
                        .foundation
                        .trip_shutdown_v1(RuntimeShutdownCauseV1::DiscordTerminal);
                    return Err(map_discord_transition_exit_v1(discord.exit()));
                }
                changed = changes.changed() => {
                    if !changed {
                        self.owner_held
                            .foundation
                            .trip_shutdown_v1(RuntimeShutdownCauseV1::ReadinessLost);
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
            self.owner_held
                .foundation
                .trip_shutdown_v1(match current.as_ref() {
                    Err(error) => gateway_observation_shutdown_cause_v1(*error),
                    Ok(_) => RuntimeShutdownCauseV1::ReadinessLost,
                });
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
        if let Some(observation) = self.owner_held.foundation.shutdown_observer_v1().observed() {
            return Err(
                RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(
                    observation.cause(),
                ),
            );
        }
        if !self
            .owner_held
            .foundation
            .startup_budget
            .operation_is_open()
        {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed);
        }
        if self.owner_held.owner.terminal_status().is_some() {
            self.owner_held
                .foundation
                .trip_shutdown_v1(RuntimeShutdownCauseV1::GatewayOwnerTerminal);
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated);
        }
        if let Some(transition) = discord_transition_failure_v1(&self.discord) {
            self.owner_held
                .foundation
                .trip_shutdown_v1(RuntimeShutdownCauseV1::DiscordTerminal);
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
        if let Some(observation) = self.owner_held.foundation.shutdown_observer_v1().observed() {
            return Err(
                RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(
                    observation.cause(),
                ),
            );
        }
        if !self
            .owner_held
            .foundation
            .startup_budget
            .operation_is_open()
        {
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::OperationDeadlineElapsed);
        }
        if self.owner_held.owner.terminal_status().is_some() {
            self.owner_held
                .foundation
                .trip_shutdown_v1(RuntimeShutdownCauseV1::GatewayOwnerTerminal);
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated);
        }
        if let Some(transition) = discord_transition_failure_v1(&self.discord) {
            self.owner_held
                .foundation
                .trip_shutdown_v1(RuntimeShutdownCauseV1::DiscordTerminal);
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
                self.owner_held
                    .foundation
                    .trip_shutdown_v1(RuntimeShutdownCauseV1::ReadinessLost);
                return Err(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(
                        RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
                    ),
                );
            }
            Err(error) => {
                self.owner_held
                    .foundation
                    .trip_shutdown_v1(gateway_observation_shutdown_cause_v1(error));
                return Err(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayObservation(error),
                );
            }
        }
        if self.owner_held.owner.terminal_status().is_some() {
            self.owner_held
                .foundation
                .trip_shutdown_v1(RuntimeShutdownCauseV1::GatewayOwnerTerminal);
            return Err(RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated);
        }
        if let Some(transition) = discord_transition_failure_v1(&self.discord) {
            self.owner_held
                .foundation
                .trip_shutdown_v1(RuntimeShutdownCauseV1::DiscordTerminal);
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

pub(super) fn gateway_observation_shutdown_cause_v1(
    error: RuntimeGatewayReadyObservationErrorV1,
) -> RuntimeShutdownCauseV1 {
    match error {
        RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain => {
            RuntimeShutdownCauseV1::GatewayOwnerTerminal
        }
        _ => RuntimeShutdownCauseV1::ReadinessLost,
    }
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
    let RuntimeOwnerHeldProcessV1 { foundation, owner } = owner_held;
    shutdown_paused_foundation_owner_v1(foundation, discord, move |deadline| {
        owner.shutdown_until(deadline)
    })
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimePausedShutdownEventV1 {
    ReadinessAndGatewayClosed,
    FinalizerJoined,
    RegistryProved,
    DiscordJoined,
    OwnerReleased,
    PoolsClosed,
    SecretsErased,
    HealthStopped,
}

struct RuntimePausedShutdownOrderV1 {
    next: u8,
}

impl RuntimePausedShutdownOrderV1 {
    const fn new() -> Self {
        Self { next: 0 }
    }

    fn record(&mut self, event: RuntimePausedShutdownEventV1) -> RuntimePausedShutdownEventV1 {
        assert_eq!(self.next, event as u8);
        self.next = self.next.saturating_add(1);
        event
    }

    fn complete(self) {
        assert_eq!(self.next, 8);
    }
}

pub(super) async fn shutdown_paused_foundation_discord_v1(
    mut foundation: super::RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
) {
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(RuntimeShutdownCauseV1::Explicit)
        .await;
    let discord_cleanup_deadline = foundation
        .startup_budget
        .discord_cleanup_deadline()
        .min(cleanup_deadline);
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation
        .gateway
        .begin_discord_drain_until_v1(discord_cleanup_deadline);
    let timing = foundation
        .lifecycle_timing_v2()
        .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin);
    let discord_shutdown = discord
        .shutdown_until(discord_drain, discord_cleanup_deadline)
        .await;
    timing.finish_v2(discord_shutdown_timing_outcome_v2(&discord_shutdown));
    let foundation_shutdown = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let outcome = if discord_shutdown.is_ok() && foundation_shutdown.is_ok() {
        RuntimeLifecycleTimingOutcomeV2::Completed
    } else {
        RuntimeLifecycleTimingOutcomeV2::FailedClosed
    };
    terminal.finish_v2(outcome);
}

pub(super) async fn shutdown_paused_foundation_owner_v1<ShutdownOwner, OwnerShutdown>(
    mut foundation: super::RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
    shutdown_owner: ShutdownOwner,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1>
where
    ShutdownOwner: FnOnce(Instant) -> OwnerShutdown,
    OwnerShutdown: Future<
        Output = Result<
            crate::RuntimeGatewayOwnerStartupWatchdogExitV1,
            crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
        >,
    >,
{
    let mut order = RuntimePausedShutdownOrderV1::new();
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(RuntimeShutdownCauseV1::Explicit)
        .await;
    let _ = order.record(RuntimePausedShutdownEventV1::ReadinessAndGatewayClosed);
    let _ = order.record(RuntimePausedShutdownEventV1::FinalizerJoined);
    let discord_cleanup_deadline = foundation
        .startup_budget
        .discord_cleanup_deadline()
        .min(cleanup_deadline);
    let owner_cleanup_deadline = foundation
        .startup_budget
        .owner_cleanup_deadline()
        .min(cleanup_deadline);
    foundation.observe_shutdown_registry_v1();
    let _ = order.record(RuntimePausedShutdownEventV1::RegistryProved);
    let discord_drain = foundation
        .gateway
        .begin_discord_drain_until_v1(discord_cleanup_deadline);
    let lifecycle_timing = foundation.lifecycle_timing_v2();
    let discord_timing =
        lifecycle_timing.start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin);
    let discord_shutdown_result = discord
        .shutdown_until(discord_drain, discord_cleanup_deadline)
        .await;
    discord_timing.finish_v2(discord_shutdown_timing_outcome_v2(&discord_shutdown_result));
    let discord_shutdown = discord_shutdown_result.map_err(map_discord_shutdown_failure_v1);
    let _ = order.record(RuntimePausedShutdownEventV1::DiscordJoined);
    let owner_timing =
        lifecycle_timing.start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin);
    let owner = shutdown_owner(owner_cleanup_deadline).await;
    owner_timing.finish_v2(owner_shutdown_timing_outcome_v2(&owner));
    let _ = order.record(RuntimePausedShutdownEventV1::OwnerReleased);
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let _ = order.record(RuntimePausedShutdownEventV1::PoolsClosed);
    let _ = order.record(RuntimePausedShutdownEventV1::SecretsErased);
    let _ = order.record(RuntimePausedShutdownEventV1::HealthStopped);
    order.complete();
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    finish_timed_paused_connected_shutdown_v2(terminal, discord_shutdown, owner_shutdown)
}

fn finish_timed_paused_connected_shutdown_v2<T>(
    terminal: RuntimeLifecycleTimingTerminalReporterV2,
    discord_shutdown: Result<T, RuntimeDiscordGatewayShutdownFailureV1>,
    owner_shutdown: Result<(), super::RuntimeOwnerHeldProcessShutdownErrorV1>,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    let result = finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown);
    let outcome = paused_connected_shutdown_timing_outcome_v2(&result);
    terminal.finish_result_v2(result, outcome)
}

fn paused_connected_shutdown_timing_outcome_v2(
    result: &Result<(), RuntimePausedConnectedProcessShutdownErrorV1>,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match result {
        Ok(()) => RuntimeLifecycleTimingOutcomeV2::Completed,
        Err(RuntimePausedConnectedProcessShutdownErrorV1::Discord(
            RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed
            | RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed,
        ))
        | Err(RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
            discord:
                RuntimeDiscordGatewayShutdownFailureV1::DeadlineElapsed
                | RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed,
            ..
        }) => RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed,
        Err(RuntimePausedConnectedProcessShutdownErrorV1::OwnerHeld(owner))
        | Err(RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
            owner_held: owner,
            ..
        }) if owner_shutdown_error_has_deadline_v2(*owner) => {
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        }
        Err(_) => RuntimeLifecycleTimingOutcomeV2::FailedClosed,
    }
}

fn owner_shutdown_error_has_deadline_v2(
    error: super::RuntimeOwnerHeldProcessShutdownErrorV1,
) -> bool {
    match error {
        super::RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwner(
            super::RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
        )
        | super::RuntimeOwnerHeldProcessShutdownErrorV1::Database(_)
        | super::RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndDatabase { .. } => true,
        super::RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndFoundation {
            owner: super::RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
            ..
        } => true,
        super::RuntimeOwnerHeldProcessShutdownErrorV1::Foundation(foundation)
        | super::RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndFoundation {
            foundation,
            ..
        } => foundation.database_only().is_some(),
        super::RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwner(_) => false,
    }
}

pub(super) fn discord_shutdown_timing_outcome_v2<T>(
    result: &Result<T, RuntimeDiscordGatewayShutdownErrorV1>,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match result {
        Ok(_) => RuntimeLifecycleTimingOutcomeV2::Completed,
        Err(
            RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed
            | RuntimeDiscordGatewayShutdownErrorV1::CloseDeadlineElapsed,
        ) => RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed,
        Err(
            RuntimeDiscordGatewayShutdownErrorV1::TaskStopped
            | RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(_),
        ) => RuntimeLifecycleTimingOutcomeV2::FailedClosed,
    }
}

pub(super) fn owner_shutdown_timing_outcome_v2(
    result: &Result<
        crate::RuntimeGatewayOwnerStartupWatchdogExitV1,
        crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    >,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match result {
        Ok(crate::RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown) => {
            RuntimeLifecycleTimingOutcomeV2::Completed
        }
        Ok(_) => RuntimeLifecycleTimingOutcomeV2::FailedClosed,
        Err(
            crate::gateway_owner_startup_watchdog::
                RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed,
        ) => RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed,
    }
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
    fn connected_outer_error_forces_failed_closed_terminal_total() {
        let (recorder, observer) =
            crate::lifecycle_timing::RuntimeLifecycleTimingRecorderV2::create_v2();
        let terminal = RuntimeLifecycleTimingTerminalReporterV2::new_v2(recorder, observer.clone());
        let discord: Result<RuntimeDiscordGatewayExitV1, _> =
            Err(RuntimeDiscordGatewayShutdownFailureV1::TaskStopped);
        assert!(finish_timed_paused_connected_shutdown_v2(terminal, discord, Ok(())).is_err());
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        );
        assert_eq!(observer.terminal_emission_count_v2(), 1);
    }

    #[test]
    fn shutdown_deadlines_preserve_deadline_timing_outcomes() {
        let discord_deadline: Result<(), _> =
            Err(RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed);
        let discord_close_deadline: Result<(), _> =
            Err(RuntimeDiscordGatewayShutdownErrorV1::CloseDeadlineElapsed);
        let owner_deadline = Err(
            crate::gateway_owner_startup_watchdog::
                RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed,
        );
        assert_eq!(
            discord_shutdown_timing_outcome_v2(&discord_deadline),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        );
        assert_eq!(
            discord_shutdown_timing_outcome_v2(&discord_close_deadline),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        );
        assert_eq!(
            owner_shutdown_timing_outcome_v2(&owner_deadline),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        );
        let (recorder, observer) =
            crate::lifecycle_timing::RuntimeLifecycleTimingRecorderV2::create_v2();
        let terminal = RuntimeLifecycleTimingTerminalReporterV2::new_v2(recorder, observer.clone());
        let discord: Result<RuntimeDiscordGatewayExitV1, _> =
            Err(RuntimeDiscordGatewayShutdownFailureV1::CloseDeadlineElapsed);
        let _ = finish_timed_paused_connected_shutdown_v2(terminal, discord, Ok(()));
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        );
        let (recorder, observer) =
            crate::lifecycle_timing::RuntimeLifecycleTimingRecorderV2::create_v2();
        let terminal = RuntimeLifecycleTimingTerminalReporterV2::new_v2(recorder, observer.clone());
        let owner = Err(RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwner(
            crate::RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
        ));
        let _ = finish_timed_paused_connected_shutdown_v2(
            terminal,
            Ok(RuntimeDiscordGatewayExitV1::Commanded),
            owner,
        );
        assert_eq!(
            observer
                .snapshot_v2()
                .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
                .unwrap()
                .outcome(),
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
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

    #[test]
    fn paused_shutdown_event_trace_is_complete_and_strictly_ordered() {
        let mut order = RuntimePausedShutdownOrderV1::new();
        let trace = [
            RuntimePausedShutdownEventV1::ReadinessAndGatewayClosed,
            RuntimePausedShutdownEventV1::FinalizerJoined,
            RuntimePausedShutdownEventV1::RegistryProved,
            RuntimePausedShutdownEventV1::DiscordJoined,
            RuntimePausedShutdownEventV1::OwnerReleased,
            RuntimePausedShutdownEventV1::PoolsClosed,
            RuntimePausedShutdownEventV1::SecretsErased,
            RuntimePausedShutdownEventV1::HealthStopped,
        ]
        .map(|event| order.record(event));
        order.complete();
        assert_eq!(
            trace,
            [
                RuntimePausedShutdownEventV1::ReadinessAndGatewayClosed,
                RuntimePausedShutdownEventV1::FinalizerJoined,
                RuntimePausedShutdownEventV1::RegistryProved,
                RuntimePausedShutdownEventV1::DiscordJoined,
                RuntimePausedShutdownEventV1::OwnerReleased,
                RuntimePausedShutdownEventV1::PoolsClosed,
                RuntimePausedShutdownEventV1::SecretsErased,
                RuntimePausedShutdownEventV1::HealthStopped,
            ]
        );
    }
}
