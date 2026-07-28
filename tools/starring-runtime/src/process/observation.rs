use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::Instant;

use automation_runtime_worker::RuntimeStartupRecoveryObservationPortV2;
pub(super) use automation_runtime_worker::{
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
};

use crate::closed_recovery::{
    RuntimeClosedRecoveryAdmissionFrozenProcessV2, RuntimeClosedRecoveryFixedPointHandoffErrorV2,
    RuntimeClosedRecoveryFixedPointV2, RuntimeClosedRecoverySessionV2,
    RuntimeClosedRecoveryStartupIterationOutcomeV2,
    RuntimeClosedRecoveryStartupObservationAttemptErrorV2,
    RuntimeClosedRecoveryStartupObservationCleanupV2,
    RuntimeClosedRecoveryStartupObservationCompletionV2,
    RuntimeClosedRecoveryStartupObservationErrorV2,
};
use crate::discord::{
    RuntimeDiscordGatewaySupervisorV1, RuntimeDiscordGatewayTerminalV1,
    RuntimeDiscordProcessHandoffFailureV2, RuntimeDiscordProcessHandoffV2,
    RuntimeDiscordProcessSupervisorV2, RuntimeDiscordShutdownOnlySupervisorV2,
};
use crate::gateway::RuntimeGatewayRecoverySectionErrorV2;
use crate::{
    RuntimeClosedRecoveryProcessCleanupFailureV2, RuntimeGatewayReadyObservationErrorV1,
    RuntimeMutationFinalizerHandoffStateV1, RuntimePausedConnectedProcessShutdownErrorV1,
    RuntimeProcessGatewayOwnerCommitFailureV2, RuntimeRegistryRecoveryObservationErrorV1,
};

use super::connected::{
    discord_transition_failure_v1, finish_paused_connected_shutdown_v1,
    map_discord_shutdown_failure_v1, map_discord_transition_exit_v1,
    shutdown_paused_foundation_owner_v1, RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::readiness::RuntimeRecoveryIterationReadyProcessV2;
use super::{RuntimeProcessFinalizerHandoffFailureV2, RuntimeProcessFoundationV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessStartupRecoveryObservationFailureV2 {
    OperationDeadlineElapsed,
    PausedConnection(RuntimeProcessPausedConnectedTransitionFailureV1),
    ObservationUnavailable,
    GatewayObservation(RuntimeGatewayReadyObservationErrorV1),
    GatewayCoordinator,
    GatewayProtocolViolation,
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    OwnerLifetime(RuntimeProcessGatewayOwnerCommitFailureV2),
}

impl RuntimeProcessStartupRecoveryObservationFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_process_startup_recovery_observation_operation_deadline_elapsed"
            }
            Self::PausedConnection(error) => error.code(),
            Self::ObservationUnavailable => {
                "runtime_process_startup_recovery_observation_unavailable"
            }
            Self::GatewayObservation(error) => error.code(),
            Self::GatewayCoordinator => {
                "runtime_process_startup_recovery_observation_gateway_coordinator"
            }
            Self::GatewayProtocolViolation => {
                "runtime_process_startup_recovery_observation_gateway_protocol_violation"
            }
            Self::Registry(error) => error.code(),
            Self::OwnerLifetime(error) => match error {
                RuntimeProcessGatewayOwnerCommitFailureV2::SafetyElapsed => {
                    "runtime_process_startup_recovery_observation_owner_safety_elapsed"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::OwnerReceiptMismatch => {
                    "runtime_process_startup_recovery_observation_owner_receipt_mismatch"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::ProtocolViolation => {
                    "runtime_process_startup_recovery_observation_owner_protocol_violation"
                }
                RuntimeProcessGatewayOwnerCommitFailureV2::SupervisorUnavailable => {
                    "runtime_process_startup_recovery_observation_owner_supervisor_unavailable"
                }
            },
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessStartupRecoveryObservationErrorV2 {
    #[error("runtime process startup recovery observation transition failed")]
    Transition(RuntimeProcessStartupRecoveryObservationFailureV2),
    #[error("runtime process startup recovery observation transition cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessStartupRecoveryObservationFailureV2,
        cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2,
    },
}

impl RuntimeProcessStartupRecoveryObservationErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_startup_recovery_observation_transition_cleanup"
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

impl Debug for RuntimeProcessStartupRecoveryObservationErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessStartupRecoveryObservationErrorV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessProductionHandoffFailureV2 {
    ProcessShutdown,
    OperationDeadlineElapsed,
    FinalizerUnavailable,
    FinalizerTerminal,
    FinalizerNotSettled,
    FixedPoint,
    Owner,
    Gateway,
    Registry,
    DiscordNotApplied,
    DiscordIndeterminate,
    ProtocolViolation,
}

impl RuntimeProcessProductionHandoffFailureV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProcessShutdown => "runtime_process_production_handoff_process_shutdown",
            Self::OperationDeadlineElapsed => {
                "runtime_process_production_handoff_operation_deadline_elapsed"
            }
            Self::FinalizerUnavailable => {
                "runtime_process_production_handoff_finalizer_unavailable"
            }
            Self::FinalizerTerminal => "runtime_process_production_handoff_finalizer_terminal",
            Self::FinalizerNotSettled => "runtime_process_production_handoff_finalizer_not_settled",
            Self::FixedPoint => "runtime_process_production_handoff_fixed_point",
            Self::Owner => "runtime_process_production_handoff_owner",
            Self::Gateway => "runtime_process_production_handoff_gateway",
            Self::Registry => "runtime_process_production_handoff_registry",
            Self::DiscordNotApplied => "runtime_process_production_handoff_discord_not_applied",
            Self::DiscordIndeterminate => {
                "runtime_process_production_handoff_discord_indeterminate"
            }
            Self::ProtocolViolation => "runtime_process_production_handoff_protocol_violation",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessProductionHandoffErrorV2 {
    #[error("runtime process production handoff failed")]
    Transition(RuntimeProcessProductionHandoffFailureV2),
    #[error("runtime process production handoff cleanup failed")]
    CleanupAfterTransition {
        transition: RuntimeProcessProductionHandoffFailureV2,
        cleanup: RuntimeClosedRecoveryProcessCleanupFailureV2,
    },
}

impl RuntimeProcessProductionHandoffErrorV2 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Transition(transition) => transition.code(),
            Self::CleanupAfterTransition { .. } => {
                "runtime_process_production_handoff_transition_cleanup"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        None
    }

    pub const fn cleanup_class(self) -> bool {
        matches!(self, Self::CleanupAfterTransition { .. })
    }
}

impl Debug for RuntimeProcessProductionHandoffErrorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessProductionHandoffErrorV2(<redacted>)")
    }
}

enum RuntimeStartupRecoveryObservationInterruptV2 {
    Discord(RuntimeProcessPausedConnectedTransitionFailureV1),
    Owner,
    Shutdown(crate::RuntimeShutdownCauseV1),
}

pub(crate) enum RuntimeStartupRecoveryObservationProcessOutcomeV2 {
    Continue(RuntimeStartupRecoveryContinueProcessV2),
    FixedPoint(RuntimeStartupRecoveryFixedPointProcessV2),
}

pub(crate) struct RuntimeStartupRecoveryContinueProcessV2 {
    pub(super) discord: RuntimeDiscordGatewaySupervisorV1,
    pub(super) foundation: RuntimeProcessFoundationV1,
    pub(super) session: RuntimeClosedRecoverySessionV2,
    pub(super) continuation: RuntimeStartupRecoveryContinuationV2,
}

pub(crate) struct RuntimeStartupRecoveryFixedPointProcessV2 {
    discord: RuntimeDiscordGatewaySupervisorV1,
    foundation: RuntimeProcessFoundationV1,
    fixed_point: RuntimeClosedRecoveryFixedPointV2,
}

pub(crate) struct RuntimePausedProductionHandoffProcessV2 {
    discord: RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    lifecycle: RuntimeClosedRecoveryAdmissionFrozenProcessV2,
    finalizer_handoff: RuntimeMutationFinalizerHandoffStateV1,
    process_generation: NonZeroU64,
}

enum RuntimeTransferredDiscordSupervisorV2 {
    Process(RuntimeDiscordProcessSupervisorV2),
    ShutdownOnly(RuntimeDiscordShutdownOnlySupervisorV2),
}

pub(crate) struct RuntimeStartupRecoveryObservedProcessV2 {
    completion: RuntimeClosedRecoveryStartupObservationCompletionV2,
}

pub(crate) struct RuntimeStartupRecoveryObservationFinalizeFailureV2 {
    discord: RuntimeDiscordGatewaySupervisorV1,
    foundation: RuntimeProcessFoundationV1,
    transition: RuntimeProcessStartupRecoveryObservationFailureV2,
    cleanup: RuntimeStartupRecoveryObservationFinalizeCleanupV2,
}

enum RuntimeStartupRecoveryObservationFinalizeCleanupV2 {
    Ready(crate::closed_recovery::RuntimeClosedRecoveryReadyIterationV2),
    Retained(RuntimeClosedRecoveryStartupObservationCleanupV2),
    Outcome(RuntimeClosedRecoveryStartupIterationOutcomeV2),
}

trait RuntimeStartupRecoveryObservationProcessStepV2<P> {
    type Observed;

    fn current_failure_v2(&self) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2>;

    fn observe_once_v2<'a>(
        &'a mut self,
        observer: &'a P,
    ) -> impl Future<
        Output = Result<Self::Observed, RuntimeProcessStartupRecoveryObservationFailureV2>,
    > + Send
           + 'a
    where
        P: 'a;
}

async fn observe_startup_recovery_process_step_v2<R, P>(
    resource: &mut R,
    observer: &P,
) -> Result<R::Observed, RuntimeProcessStartupRecoveryObservationFailureV2>
where
    R: RuntimeStartupRecoveryObservationProcessStepV2<P>,
{
    if let Some(transition) = resource.current_failure_v2() {
        return Err(transition);
    }
    let observed = resource.observe_once_v2(observer).await?;
    if let Some(transition) = resource.current_failure_v2() {
        return Err(transition);
    }
    Ok(observed)
}

fn finalize_startup_recovery_process_step_v2<
    Resource,
    Observed,
    Outcome,
    Failure,
    CurrentResource,
    RetainResource,
    Finalize,
    CurrentOutcome,
    RetainOutcome,
>(
    resource: Resource,
    observed: Observed,
    current_resource: CurrentResource,
    retain_resource: RetainResource,
    finalize: Finalize,
    current_outcome: CurrentOutcome,
    retain_outcome: RetainOutcome,
) -> Result<Outcome, Failure>
where
    CurrentResource: FnOnce(&Resource) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2>,
    RetainResource: FnOnce(Resource, RuntimeProcessStartupRecoveryObservationFailureV2) -> Failure,
    Finalize: FnOnce(Resource, Observed) -> Result<Outcome, Failure>,
    CurrentOutcome: FnOnce(&Outcome) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2>,
    RetainOutcome: FnOnce(Outcome, RuntimeProcessStartupRecoveryObservationFailureV2) -> Failure,
{
    if let Some(transition) = current_resource(&resource) {
        return Err(retain_resource(resource, transition));
    }
    let outcome = finalize(resource, observed)?;
    if let Some(transition) = current_outcome(&outcome) {
        return Err(retain_outcome(outcome, transition));
    }
    Ok(outcome)
}

impl<P> RuntimeStartupRecoveryObservationProcessStepV2<P> for RuntimeRecoveryIterationReadyProcessV2
where
    P: RuntimeStartupRecoveryObservationPortV2 + Sync,
{
    type Observed = RuntimeStartupRecoveryObservedProcessV2;

    fn current_failure_v2(&self) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2> {
        current_observation_transition_v2(&self.foundation, &self.discord, &self.iteration)
    }

    #[expect(
        clippy::manual_async_fn,
        reason = "the private seam preserves a Send future contract"
    )]
    fn observe_once_v2<'a>(
        &'a mut self,
        observer: &'a P,
    ) -> impl Future<
        Output = Result<Self::Observed, RuntimeProcessStartupRecoveryObservationFailureV2>,
    > + Send
           + 'a
    where
        P: 'a,
    {
        async move {
            let Self {
                discord,
                foundation,
                iteration,
            } = self;
            let operation_cutoff = foundation.startup_budget.operation_cutoff();
            let owner_safety_deadline = iteration.owner_safety_deadline_v2();
            let owner_terminal = iteration.owner_terminal_observation_v2();
            let discord_terminal = async {
                let transition =
                    map_discord_transition_exit_v1(discord.wait_terminal().await.exit());
                foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
                transition
            };
            let owner_terminal = async {
                let _ = owner_terminal.await;
                foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
            };
            let mut shutdown = foundation.shutdown_observer_v1();
            let interrupt = await_startup_recovery_observation_interrupt_v2(
                discord_terminal,
                owner_terminal,
                async move { shutdown.wait().await },
            );
            let completion = iteration
                .observe_startup_recovery_interruptible_in_place_v2(observer, interrupt)
                .await
                .map_err(|attempt| {
                    map_observation_attempt_failure_v2(
                        attempt,
                        operation_cutoff,
                        owner_safety_deadline,
                    )
                })?;
            Ok(RuntimeStartupRecoveryObservedProcessV2 { completion })
        }
    }
}

impl RuntimeRecoveryIterationReadyProcessV2 {
    pub(crate) async fn observe_startup_recovery_once_v2<P>(
        &mut self,
        observer: &P,
    ) -> Result<
        RuntimeStartupRecoveryObservedProcessV2,
        RuntimeProcessStartupRecoveryObservationFailureV2,
    >
    where
        P: RuntimeStartupRecoveryObservationPortV2 + Sync,
    {
        observe_startup_recovery_process_step_v2(self, observer).await
    }

    pub(crate) fn into_startup_recovery_observation_outcome_v2(
        self,
        observed: RuntimeStartupRecoveryObservedProcessV2,
    ) -> Result<
        RuntimeStartupRecoveryObservationProcessOutcomeV2,
        Box<RuntimeStartupRecoveryObservationFinalizeFailureV2>,
    > {
        finalize_startup_recovery_process_step_v2(
            self,
            observed,
            current_ready_process_observation_transition_v2,
            retain_ready_process_observation_failure_v2,
            finalize_observed_startup_recovery_process_v2,
            current_typed_observation_outcome_transition_v2,
            retain_typed_observation_outcome_failure_v2,
        )
    }

    pub(crate) async fn cleanup_after_startup_recovery_observation_failure_v2(
        self,
        transition: RuntimeProcessStartupRecoveryObservationFailureV2,
    ) -> RuntimeProcessStartupRecoveryObservationErrorV2 {
        let Self {
            discord,
            foundation,
            iteration,
        } = self;
        let cleanup =
            shutdown_startup_observation_process_v2(foundation, discord, move |cleanup_deadline| {
                iteration.abort_and_shutdown_until_v2(cleanup_deadline)
            })
            .await;
        finish_observation_transition_v2(transition, cleanup)
    }
}

fn current_ready_process_observation_transition_v2(
    process: &RuntimeRecoveryIterationReadyProcessV2,
) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2> {
    current_observation_transition_v2(&process.foundation, &process.discord, &process.iteration)
}

fn retain_ready_process_observation_failure_v2(
    process: RuntimeRecoveryIterationReadyProcessV2,
    transition: RuntimeProcessStartupRecoveryObservationFailureV2,
) -> Box<RuntimeStartupRecoveryObservationFinalizeFailureV2> {
    let RuntimeRecoveryIterationReadyProcessV2 {
        discord,
        foundation,
        iteration,
    } = process;
    Box::new(RuntimeStartupRecoveryObservationFinalizeFailureV2 {
        discord,
        foundation,
        transition,
        cleanup: RuntimeStartupRecoveryObservationFinalizeCleanupV2::Ready(iteration),
    })
}

fn finalize_observed_startup_recovery_process_v2(
    process: RuntimeRecoveryIterationReadyProcessV2,
    observed: RuntimeStartupRecoveryObservedProcessV2,
) -> Result<
    RuntimeStartupRecoveryObservationProcessOutcomeV2,
    Box<RuntimeStartupRecoveryObservationFinalizeFailureV2>,
> {
    let operation_cutoff = process.foundation.startup_budget.operation_cutoff();
    let owner_safety_deadline = process.iteration.owner_safety_deadline_v2();
    let RuntimeRecoveryIterationReadyProcessV2 {
        discord,
        foundation,
        iteration,
    } = process;
    let outcome = match iteration.into_startup_recovery_observation_outcome_v2(observed.completion)
    {
        Ok(outcome) => outcome,
        Err(failure) => {
            let (attempt, cleanup) = (*failure).into_parts();
            let transition = map_infallible_observation_attempt_failure_v2(
                attempt,
                operation_cutoff,
                owner_safety_deadline,
            );
            return Err(Box::new(
                RuntimeStartupRecoveryObservationFinalizeFailureV2 {
                    discord,
                    foundation,
                    transition,
                    cleanup: RuntimeStartupRecoveryObservationFinalizeCleanupV2::Retained(cleanup),
                },
            ));
        }
    };
    Ok(match outcome {
        RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
            session,
            continuation,
        } => RuntimeStartupRecoveryObservationProcessOutcomeV2::Continue(
            RuntimeStartupRecoveryContinueProcessV2 {
                discord,
                foundation,
                session,
                continuation,
            },
        ),
        RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(fixed_point) => {
            RuntimeStartupRecoveryObservationProcessOutcomeV2::FixedPoint(
                RuntimeStartupRecoveryFixedPointProcessV2 {
                    discord,
                    foundation,
                    fixed_point,
                },
            )
        }
    })
}

fn current_typed_observation_outcome_transition_v2(
    outcome: &RuntimeStartupRecoveryObservationProcessOutcomeV2,
) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2> {
    match outcome {
        RuntimeStartupRecoveryObservationProcessOutcomeV2::Continue(process) => {
            current_observation_lifetime_transition_v2(
                &process.foundation,
                &process.discord,
                process.session.owner_terminal_status_v2().is_some()
                    || Instant::now() >= process.session.owner_safety_deadline_v2(),
            )
        }
        RuntimeStartupRecoveryObservationProcessOutcomeV2::FixedPoint(process) => {
            current_observation_lifetime_transition_v2(
                &process.foundation,
                &process.discord,
                process.fixed_point.owner_terminal_status_v2().is_some()
                    || Instant::now() >= process.fixed_point.owner_safety_deadline_v2(),
            )
        }
    }
}

fn retain_typed_observation_outcome_failure_v2(
    outcome: RuntimeStartupRecoveryObservationProcessOutcomeV2,
    transition: RuntimeProcessStartupRecoveryObservationFailureV2,
) -> Box<RuntimeStartupRecoveryObservationFinalizeFailureV2> {
    let (discord, foundation, outcome) = match outcome {
        RuntimeStartupRecoveryObservationProcessOutcomeV2::Continue(process) => (
            process.discord,
            process.foundation,
            RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
                session: process.session,
                continuation: process.continuation,
            },
        ),
        RuntimeStartupRecoveryObservationProcessOutcomeV2::FixedPoint(process) => (
            process.discord,
            process.foundation,
            RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(process.fixed_point),
        ),
    };
    Box::new(RuntimeStartupRecoveryObservationFinalizeFailureV2 {
        discord,
        foundation,
        transition,
        cleanup: RuntimeStartupRecoveryObservationFinalizeCleanupV2::Outcome(outcome),
    })
}

impl RuntimeStartupRecoveryObservationFinalizeFailureV2 {
    pub(crate) async fn cleanup(self) -> RuntimeProcessStartupRecoveryObservationErrorV2 {
        let Self {
            discord,
            foundation,
            transition,
            cleanup,
        } = self;
        let cleanup = match cleanup {
            RuntimeStartupRecoveryObservationFinalizeCleanupV2::Ready(iteration) => {
                shutdown_startup_observation_process_v2(
                    foundation,
                    discord,
                    move |cleanup_deadline| iteration.abort_and_shutdown_until_v2(cleanup_deadline),
                )
                .await
            }
            RuntimeStartupRecoveryObservationFinalizeCleanupV2::Retained(owner) => {
                shutdown_startup_observation_process_v2(
                    foundation,
                    discord,
                    move |cleanup_deadline| owner.abort_and_shutdown_until_v2(cleanup_deadline),
                )
                .await
            }
            RuntimeStartupRecoveryObservationFinalizeCleanupV2::Outcome(outcome) => {
                shutdown_startup_recovery_outcome_v2(foundation, discord, outcome).await
            }
        };
        finish_observation_transition_v2(transition, cleanup)
    }
}

impl RuntimeStartupRecoveryContinueProcessV2 {
    pub(crate) fn continuation_v2(&self) -> RuntimeStartupRecoveryContinuationV2 {
        self.continuation
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        shutdown_startup_observation_process_v2(
            self.foundation,
            self.discord,
            move |cleanup_deadline| self.session.abort_and_shutdown_until_v2(cleanup_deadline),
        )
        .await
        .map_err(Into::into)
    }
}

impl RuntimeStartupRecoveryFixedPointProcessV2 {
    pub(crate) async fn into_paused_production_handoff_v2(
        mut self,
    ) -> Result<RuntimePausedProductionHandoffProcessV2, RuntimeProcessProductionHandoffErrorV2>
    {
        if let Err(error) = self.fixed_point.revalidate_for_handoff_v2() {
            let transition = map_fixed_point_handoff_failure_v2(error);
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        let handoff_cutoff = self.fixed_point.handoff_cutoff_v2();
        let mut shutdown = self.foundation.shutdown_observer_v1();
        let finalizer_result = await_production_handoff_stage_v2(
            self.foundation
                .seal_startup_finalizer_for_handoff_v2(handoff_cutoff),
            &mut shutdown,
        )
        .await
        .ok_or(RuntimeProcessProductionHandoffFailureV2::ProcessShutdown)
        .and_then(|result| result.map_err(map_finalizer_handoff_failure_v2));
        let finalizer_handoff = match finalizer_result {
            Ok(handoff) => handoff,
            Err(transition) => {
                let cleanup = self.shutdown().await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let Self {
            discord,
            foundation,
            fixed_point,
        } = self;
        let mut worker_fixed_point = match fixed_point.into_worker_fixed_point_v2() {
            Ok(fixed_point) => fixed_point,
            Err(failure) => {
                let transition = map_fixed_point_handoff_failure_v2(failure.error_v2());
                let fixed_point = failure.into_fixed_point_v2();
                let cleanup = shutdown_startup_observation_process_v2(
                    foundation,
                    discord,
                    move |cleanup_deadline| {
                        fixed_point.abort_and_shutdown_until_v2(cleanup_deadline)
                    },
                )
                .await
                .map_err(Into::into);
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let mut shutdown = foundation.shutdown_observer_v1();
        let freeze_result = await_production_handoff_stage_v2(
            worker_fixed_point.enter_admission_frozen_in_place_v2(),
            &mut shutdown,
        )
        .await
        .ok_or(RuntimeProcessProductionHandoffFailureV2::ProcessShutdown)
        .and_then(|result| result.map_err(map_fixed_point_handoff_failure_v2));
        if let Err(transition) = freeze_result {
            let cleanup = shutdown_startup_observation_process_v2(
                foundation,
                discord,
                move |cleanup_deadline| {
                    worker_fixed_point.abort_and_shutdown_until_v2(cleanup_deadline)
                },
            )
            .await
            .map_err(Into::into);
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        let lifecycle = match worker_fixed_point.try_into_admission_frozen_v2() {
            Ok(lifecycle) => lifecycle,
            Err(failure) => {
                let transition = map_fixed_point_handoff_failure_v2(failure.error_v2());
                let cleanup = shutdown_startup_observation_process_v2(
                    foundation,
                    discord,
                    move |cleanup_deadline| failure.abort_and_shutdown_until_v2(cleanup_deadline),
                )
                .await
                .map_err(Into::into);
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let process_generation = NonZeroU64::MIN;
        let mut discord = discord;
        let mut shutdown = foundation.shutdown_observer_v1();
        let shutdown_interrupted = await_production_handoff_stage_v2(
            discord.handoff_to_process_in_place_v2(process_generation),
            &mut shutdown,
        )
        .await
        .is_none();
        match discord.into_process_handoff_v2() {
            RuntimeDiscordProcessHandoffV2::Process(discord) => {
                let process = RuntimePausedProductionHandoffProcessV2 {
                    discord,
                    foundation,
                    lifecycle,
                    finalizer_handoff,
                    process_generation,
                };
                if shutdown_interrupted {
                    let cleanup = process.shutdown().await;
                    return Err(finish_production_handoff_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
                        cleanup,
                    ));
                }
                if let Err(transition) = process.revalidate_paused_v2() {
                    let cleanup = process.shutdown().await;
                    return Err(finish_production_handoff_transition_v2(transition, cleanup));
                }
                Ok(process)
            }
            RuntimeDiscordProcessHandoffV2::NotApplied {
                supervisor,
                failure,
            } => {
                let transition = if shutdown_interrupted {
                    RuntimeProcessProductionHandoffFailureV2::ProcessShutdown
                } else {
                    map_discord_handoff_failure_v2(failure, false)
                };
                let cleanup = shutdown_startup_observation_process_v2(
                    foundation,
                    supervisor,
                    move |cleanup_deadline| lifecycle.abort_and_shutdown_until_v2(cleanup_deadline),
                )
                .await
                .map_err(Into::into);
                Err(finish_production_handoff_transition_v2(transition, cleanup))
            }
            RuntimeDiscordProcessHandoffV2::Indeterminate {
                supervisor,
                failure,
            } => {
                let transition = if shutdown_interrupted {
                    RuntimeProcessProductionHandoffFailureV2::ProcessShutdown
                } else {
                    map_discord_handoff_failure_v2(failure, true)
                };
                let cleanup = shutdown_transferred_production_handoff_v2(
                    foundation,
                    RuntimeTransferredDiscordSupervisorV2::ShutdownOnly(supervisor),
                    lifecycle,
                    process_generation,
                )
                .await;
                Err(finish_production_handoff_transition_v2(transition, cleanup))
            }
        }
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        shutdown_startup_observation_process_v2(
            self.foundation,
            self.discord,
            move |cleanup_deadline| {
                self.fixed_point
                    .abort_and_shutdown_until_v2(cleanup_deadline)
            },
        )
        .await
        .map_err(Into::into)
    }
}

async fn await_production_handoff_stage_v2<Stage, Output>(
    stage: Stage,
    shutdown: &mut crate::shutdown::RuntimeShutdownObserverV1,
) -> Option<Output>
where
    Stage: Future<Output = Output>,
{
    tokio::pin!(stage);
    tokio::select! {
        biased;
        _observation = shutdown.wait() => None,
        output = &mut stage => Some(output),
    }
}

impl RuntimePausedProductionHandoffProcessV2 {
    pub(crate) fn revalidate_paused_v2(
        &self,
    ) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate);
        }
        self.foundation
            .revalidate_finalizer_handoff_v2(self.finalizer_handoff)
            .map_err(map_finalizer_handoff_failure_v2)?;
        self.lifecycle
            .revalidate_paused_v2()
            .map_err(map_fixed_point_handoff_failure_v2)?;
        production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        shutdown_transferred_production_handoff_v2(
            self.foundation,
            RuntimeTransferredDiscordSupervisorV2::Process(self.discord),
            self.lifecycle,
            self.process_generation,
        )
        .await
    }
}

fn production_handoff_shutdown_failure_v2(
    shutdown: &crate::shutdown::RuntimeShutdownObserverV1,
) -> Option<RuntimeProcessProductionHandoffFailureV2> {
    shutdown
        .observed()
        .map(|_| RuntimeProcessProductionHandoffFailureV2::ProcessShutdown)
}

impl RuntimeTransferredDiscordSupervisorV2 {
    async fn shutdown_until_v2<F>(
        self,
        begin_drain: F,
        shutdown_generation: NonZeroU64,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDiscordGatewayTerminalV1, crate::discord::RuntimeDiscordGatewayShutdownErrorV1>
    where
        F: Future<Output = bool>,
    {
        match self {
            Self::Process(supervisor) => {
                supervisor
                    .shutdown_until(begin_drain, shutdown_generation, cleanup_deadline)
                    .await
            }
            Self::ShutdownOnly(supervisor) => {
                supervisor
                    .shutdown_until(begin_drain, shutdown_generation, cleanup_deadline)
                    .await
            }
        }
    }
}

async fn shutdown_transferred_production_handoff_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeTransferredDiscordSupervisorV2,
    lifecycle: RuntimeClosedRecoveryAdmissionFrozenProcessV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let cleanup_deadline = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    let discord_cleanup_deadline = foundation
        .startup_budget
        .discord_cleanup_deadline()
        .min(cleanup_deadline);
    let owner_cleanup_deadline = foundation
        .startup_budget
        .owner_cleanup_deadline()
        .min(cleanup_deadline);
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let discord_shutdown = discord
        .shutdown_until_v2(discord_drain, process_generation, discord_cleanup_deadline)
        .await
        .map_err(map_discord_shutdown_failure_v1);
    let owner = lifecycle
        .abort_and_shutdown_until_v2(owner_cleanup_deadline)
        .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into)
}

fn finish_production_handoff_transition_v2(
    transition: RuntimeProcessProductionHandoffFailureV2,
    cleanup: Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2>,
) -> RuntimeProcessProductionHandoffErrorV2 {
    match cleanup {
        Ok(()) => RuntimeProcessProductionHandoffErrorV2::Transition(transition),
        Err(cleanup) => RuntimeProcessProductionHandoffErrorV2::CleanupAfterTransition {
            transition,
            cleanup,
        },
    }
}

fn map_finalizer_handoff_failure_v2(
    error: RuntimeProcessFinalizerHandoffFailureV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match error {
        RuntimeProcessFinalizerHandoffFailureV2::DeadlineElapsed => {
            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed
        }
        RuntimeProcessFinalizerHandoffFailureV2::Unavailable => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerUnavailable
        }
        RuntimeProcessFinalizerHandoffFailureV2::Terminal(_) => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal
        }
        RuntimeProcessFinalizerHandoffFailureV2::NotSettled => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerNotSettled
        }
    }
}

fn map_fixed_point_handoff_failure_v2(
    error: RuntimeClosedRecoveryFixedPointHandoffErrorV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match error {
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::DeadlineElapsed => {
            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed
        }
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::Recovery(_)
        | RuntimeClosedRecoveryFixedPointHandoffErrorV2::Gateway(_) => {
            RuntimeProcessProductionHandoffFailureV2::FixedPoint
        }
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::GatewayObservation(_) => {
            RuntimeProcessProductionHandoffFailureV2::Gateway
        }
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::Registry(_) => {
            RuntimeProcessProductionHandoffFailureV2::Registry
        }
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::Owner(_) => {
            RuntimeProcessProductionHandoffFailureV2::Owner
        }
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::ProtocolViolation => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
    }
}

fn map_discord_handoff_failure_v2(
    _error: RuntimeDiscordProcessHandoffFailureV2,
    indeterminate: bool,
) -> RuntimeProcessProductionHandoffFailureV2 {
    if indeterminate {
        RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate
    } else {
        RuntimeProcessProductionHandoffFailureV2::DiscordNotApplied
    }
}

impl Debug for RuntimeStartupRecoveryObservationProcessOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryObservationProcessOutcomeV2(<redacted>)")
    }
}

impl Debug for RuntimeStartupRecoveryContinueProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryContinueProcessV2(<redacted>)")
    }
}

impl Debug for RuntimeStartupRecoveryFixedPointProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryFixedPointProcessV2(<redacted>)")
    }
}

impl Debug for RuntimeStartupRecoveryObservedProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryObservedProcessV2(<redacted>)")
    }
}

impl Debug for RuntimeStartupRecoveryObservationFinalizeFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryObservationFinalizeFailureV2(<redacted>)")
    }
}

fn current_observation_transition_v2(
    foundation: &RuntimeProcessFoundationV1,
    discord: &RuntimeDiscordGatewaySupervisorV1,
    iteration: &crate::closed_recovery::RuntimeClosedRecoveryReadyIterationV2,
) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2> {
    current_observation_lifetime_transition_v2(
        foundation,
        discord,
        iteration.owner_terminal_status_v2().is_some()
            || Instant::now() >= iteration.owner_safety_deadline_v2(),
    )
}

fn current_observation_lifetime_transition_v2(
    foundation: &RuntimeProcessFoundationV1,
    discord: &RuntimeDiscordGatewaySupervisorV1,
    owner_unavailable: bool,
) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2> {
    if let Some(error) = discord_transition_failure_v1(discord) {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::DiscordTerminal);
        return Some(RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(error));
    }
    if owner_unavailable {
        foundation.trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
        return Some(
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        );
    }
    if !foundation.startup_budget.operation_is_open() {
        return Some(RuntimeProcessStartupRecoveryObservationFailureV2::OperationDeadlineElapsed);
    }
    None
}

fn map_observation_attempt_failure_v2<E>(
    attempt: RuntimeClosedRecoveryStartupObservationAttemptErrorV2<
        E,
        RuntimeStartupRecoveryObservationInterruptV2,
    >,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessStartupRecoveryObservationFailureV2 {
    match attempt {
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(error) => {
            map_observation_failure_v2(error, operation_cutoff, owner_safety_deadline)
        }
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(
            RuntimeStartupRecoveryObservationInterruptV2::Discord(error),
        ) => RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(error),
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(
            RuntimeStartupRecoveryObservationInterruptV2::Owner,
        ) => RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        ),
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(
            RuntimeStartupRecoveryObservationInterruptV2::Shutdown(cause),
        ) => RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(cause),
        ),
    }
}

fn map_infallible_observation_attempt_failure_v2(
    attempt: RuntimeClosedRecoveryStartupObservationAttemptErrorV2<
        std::convert::Infallible,
        std::convert::Infallible,
    >,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessStartupRecoveryObservationFailureV2 {
    match attempt {
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(error) => {
            map_observation_failure_v2(error, operation_cutoff, owner_safety_deadline)
        }
        RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(never) => match never {},
    }
}

fn map_observation_failure_v2<E>(
    error: RuntimeClosedRecoveryStartupObservationErrorV2<E>,
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessStartupRecoveryObservationFailureV2 {
    match error {
        RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed => {
            classify_observation_deadline_v2(operation_cutoff, owner_safety_deadline)
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Observer(_) => {
            RuntimeProcessStartupRecoveryObservationFailureV2::ObservationUnavailable
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(error) => {
            map_gateway_observation_failure_v2(error)
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Registry(error) => {
            RuntimeProcessStartupRecoveryObservationFailureV2::Registry(error)
        }
        RuntimeClosedRecoveryStartupObservationErrorV2::Owner(error) => {
            RuntimeProcessStartupRecoveryObservationFailureV2::OwnerLifetime(error.into())
        }
    }
}

fn map_gateway_observation_failure_v2(
    error: RuntimeGatewayRecoverySectionErrorV2,
) -> RuntimeProcessStartupRecoveryObservationFailureV2 {
    match error {
        RuntimeGatewayRecoverySectionErrorV2::Gateway(error) => {
            RuntimeProcessStartupRecoveryObservationFailureV2::GatewayObservation(error)
        }
        RuntimeGatewayRecoverySectionErrorV2::Coordinator(_) => {
            RuntimeProcessStartupRecoveryObservationFailureV2::GatewayCoordinator
        }
        RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation => {
            RuntimeProcessStartupRecoveryObservationFailureV2::GatewayProtocolViolation
        }
    }
}

fn classify_observation_deadline_v2(
    operation_cutoff: Instant,
    owner_safety_deadline: Instant,
) -> RuntimeProcessStartupRecoveryObservationFailureV2 {
    if owner_safety_deadline <= operation_cutoff {
        RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    } else {
        RuntimeProcessStartupRecoveryObservationFailureV2::OperationDeadlineElapsed
    }
}

async fn await_startup_recovery_observation_interrupt_v2<DiscordTerminal, OwnerTerminal, Shutdown>(
    discord_terminal: DiscordTerminal,
    owner_terminal: OwnerTerminal,
    shutdown: Shutdown,
) -> RuntimeStartupRecoveryObservationInterruptV2
where
    DiscordTerminal: Future<Output = RuntimeProcessPausedConnectedTransitionFailureV1>,
    OwnerTerminal: Future<Output = ()>,
    Shutdown: Future<Output = crate::RuntimeShutdownObservationV1>,
{
    tokio::pin!(discord_terminal);
    tokio::pin!(owner_terminal);
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        observation = &mut shutdown => {
            RuntimeStartupRecoveryObservationInterruptV2::Shutdown(observation.cause())
        }
        transition = &mut discord_terminal => {
            RuntimeStartupRecoveryObservationInterruptV2::Discord(transition)
        }
        () = &mut owner_terminal => {
            RuntimeStartupRecoveryObservationInterruptV2::Owner
        }
    }
}

async fn shutdown_startup_recovery_outcome_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordGatewaySupervisorV1,
    outcome: RuntimeClosedRecoveryStartupIterationOutcomeV2,
) -> Result<(), RuntimePausedConnectedProcessShutdownErrorV1> {
    match outcome {
        RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue { session, .. } => {
            shutdown_startup_observation_process_v2(foundation, discord, move |cleanup_deadline| {
                session.abort_and_shutdown_until_v2(cleanup_deadline)
            })
            .await
        }
        RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(fixed_point) => {
            shutdown_startup_observation_process_v2(foundation, discord, move |cleanup_deadline| {
                fixed_point.abort_and_shutdown_until_v2(cleanup_deadline)
            })
            .await
        }
    }
}

#[cfg(test)]
async fn sequence_startup_observation_cleanup_v2<
    StartDiscord,
    DiscordShutdown,
    DiscordResult,
    StartOwner,
    OwnerShutdown,
    OwnerResult,
    StartDatabase,
    DatabaseShutdown,
    DatabaseResult,
    FinishOwnerHeld,
    OwnerHeldResult,
    Finish,
    Result,
>(
    start_discord: StartDiscord,
    start_owner: StartOwner,
    start_database: StartDatabase,
    finish_owner_held: FinishOwnerHeld,
    finish: Finish,
) -> Result
where
    StartDiscord: FnOnce() -> DiscordShutdown,
    DiscordShutdown: Future<Output = DiscordResult>,
    StartOwner: FnOnce() -> OwnerShutdown,
    OwnerShutdown: Future<Output = OwnerResult>,
    StartDatabase: FnOnce() -> DatabaseShutdown,
    DatabaseShutdown: Future<Output = DatabaseResult>,
    FinishOwnerHeld: FnOnce(OwnerResult, DatabaseResult) -> OwnerHeldResult,
    Finish: FnOnce(DiscordResult, OwnerHeldResult) -> Result,
{
    let discord = start_discord().await;
    let owner = start_owner().await;
    let database = start_database().await;
    finish(discord, finish_owner_held(owner, database))
}

async fn shutdown_startup_observation_process_v2<ShutdownOwner, OwnerShutdown>(
    foundation: RuntimeProcessFoundationV1,
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
    shutdown_paused_foundation_owner_v1(foundation, discord, shutdown_owner).await
}

fn finish_observation_transition_v2(
    transition: RuntimeProcessStartupRecoveryObservationFailureV2,
    cleanup: Result<(), RuntimePausedConnectedProcessShutdownErrorV1>,
) -> RuntimeProcessStartupRecoveryObservationErrorV2 {
    match cleanup {
        Ok(()) => RuntimeProcessStartupRecoveryObservationErrorV2::Transition(transition),
        Err(cleanup) => RuntimeProcessStartupRecoveryObservationErrorV2::CleanupAfterTransition {
            transition,
            cleanup: cleanup.into(),
        },
    }
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
