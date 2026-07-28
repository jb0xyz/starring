use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeIngressOpenAcknowledgementLeaseDurationV2, RuntimeObserveWriterFenceV1,
    RuntimeWriterFenceObservationV1,
};
use automation_runtime_worker::{
    RuntimeEmptyOpenAcknowledgementRefreshInputV2, RuntimeIngressOpenAcknowledgementPortV2,
    RuntimeOpenProductionObservationInputV2, RuntimeStartupRecoveryObservationPortV2,
    RuntimeWriterFenceObservationPortV1,
};
pub(super) use automation_runtime_worker::{
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
};

use crate::closed_recovery::{
    RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    RuntimeClosedRecoveryAdmissionFrozenProcessV2, RuntimeClosedRecoveryEmptyOpenProcessV2,
    RuntimeClosedRecoveryFixedPointHandoffErrorV2, RuntimeClosedRecoveryFixedPointV2,
    RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2,
    RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2,
    RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2,
    RuntimeClosedRecoveryProcessFrozenProcessV2, RuntimeClosedRecoveryProductionHandoffErrorV2,
    RuntimeClosedRecoveryProductionHandoffProcessV2, RuntimeClosedRecoveryResumeObservationV2,
    RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryStartupIterationOutcomeV2,
    RuntimeClosedRecoveryStartupObservationAttemptErrorV2,
    RuntimeClosedRecoveryStartupObservationCleanupV2,
    RuntimeClosedRecoveryStartupObservationCompletionV2,
    RuntimeClosedRecoveryStartupObservationErrorV2,
    RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
};
use crate::discord::{
    RuntimeDiscordGatewaySupervisorV1, RuntimeDiscordGatewayTerminalV1,
    RuntimeDiscordProcessHandoffFailureV2, RuntimeDiscordProcessHandoffV2,
    RuntimeDiscordProcessSupervisorV2, RuntimeDiscordRecoveryResumeAttemptV2,
    RuntimeDiscordShutdownOnlySupervisorV2,
};
use crate::gateway::RuntimeGatewayRecoverySectionErrorV2;
use crate::ingress_acknowledgement_safety::RuntimeIngressAcknowledgementSafetyMonitorV2;
use crate::ingress_acknowledgement_supervisor::{
    RuntimeIngressAcknowledgementExecutionResultV2, RuntimeIngressAcknowledgementFailureV2,
    RuntimeIngressAcknowledgementRegistrationRejectionReasonV2,
    RuntimeWorkerIngressAcknowledgementJobV2,
};
use crate::lifecycle_timing::{
    RuntimeLifecycleTimingMetricV2, RuntimeLifecycleTimingOutcomeV2,
    RuntimeLifecycleTimingRecorderV2, RuntimeLifecycleTimingTerminalReporterV2,
};
use crate::maintenance_ingress_gate::{
    RuntimeMaintenanceIngressGateOpenAuthorityV2, RuntimeMaintenanceIngressGateSnapshotV2,
    RuntimeMaintenanceIngressGateStageV2,
};
use crate::{
    RuntimeClosedRecoveryProcessCleanupFailureV2, RuntimeGatewayOwnerStartupWatchdogExitV1,
    RuntimeGatewayReadyObservationErrorV1, RuntimeMutationFinalizerHandoffStateV1,
    RuntimePausedConnectedProcessShutdownErrorV1, RuntimeProcessGatewayOwnerCommitFailureV2,
    RuntimeRegistryRecoveryObservationErrorV1,
};

use super::connected::{
    discord_shutdown_timing_outcome_v2, discord_transition_failure_v1,
    finish_paused_connected_shutdown_v1, map_discord_shutdown_failure_v1,
    map_discord_transition_exit_v1, owner_shutdown_timing_outcome_v2,
    shutdown_paused_foundation_owner_v1, RuntimeProcessPausedConnectedTransitionFailureV1,
};
use super::readiness::RuntimeRecoveryIterationReadyProcessV2;
use super::{
    RuntimeProcessFinalizerActivationFailureV2, RuntimeProcessFinalizerHandoffFailureV2,
    RuntimeProcessFoundationV1, RuntimeProcessIngressAcknowledgementSupervisorV2,
};

const INGRESS_ACKNOWLEDGEMENT_LEASE_V2: Duration = Duration::from_secs(10);
const INGRESS_ACKNOWLEDGEMENT_REFRESH_ADVANCE_V2: Duration = Duration::from_secs(5);
const INGRESS_ACKNOWLEDGEMENT_SAFETY_MARGIN_V2: Duration = Duration::from_secs(2);

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
    FinalizerActivation,
    ProcessCleanupMode,
    Database,
    MaintenanceGate,
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
            Self::FinalizerActivation => "runtime_process_production_handoff_finalizer_activation",
            Self::ProcessCleanupMode => "runtime_process_production_handoff_process_cleanup_mode",
            Self::Database => "runtime_process_production_handoff_database",
            Self::MaintenanceGate => "runtime_process_production_handoff_maintenance_gate",
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

pub(crate) struct RuntimeProcessBoundProductionHandoffProcessV2 {
    discord: RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    lifecycle: RuntimeClosedRecoveryProcessFrozenProcessV2,
    finalizer_generation: automation_runtime_worker::RuntimeMutationFinalizerGenerationV1,
    process_generation: NonZeroU64,
}

pub(crate) struct RuntimeRecoveryResumeProcessV2 {
    discord: RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    lifecycle: RuntimeClosedRecoveryProductionHandoffProcessV2,
    process_generation: NonZeroU64,
}

pub(crate) struct RuntimeAdmissionAcknowledgingProcessV2 {
    discord: RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    lifecycle: RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    process_generation: NonZeroU64,
}

pub(crate) struct RuntimeEmptyOpenProcessV2 {
    discord: RuntimeDiscordProcessSupervisorV2,
    foundation: RuntimeProcessFoundationV1,
    lifecycle: RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    acknowledgement_schedule: RuntimeIngressAcknowledgementScheduleV2,
    acknowledgement_safety: RuntimeIngressAcknowledgementSafetyMonitorV2,
    process_generation: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeIngressAcknowledgementScheduleV2 {
    refresh_at: Instant,
    safety_deadline: Instant,
}

struct RuntimeIngressAcknowledgementCleanupV2 {
    supervisor: RuntimeProcessIngressAcknowledgementSupervisorV2,
    safety: Option<RuntimeIngressAcknowledgementSafetyMonitorV2>,
}

impl RuntimeIngressAcknowledgementCleanupV2 {
    fn new_v2(
        supervisor: RuntimeProcessIngressAcknowledgementSupervisorV2,
        safety: Option<RuntimeIngressAcknowledgementSafetyMonitorV2>,
    ) -> Self {
        Self { supervisor, safety }
    }
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

    pub(crate) async fn into_process_bound_handoff_v2(
        mut self,
    ) -> Result<RuntimeProcessBoundProductionHandoffProcessV2, RuntimeProcessProductionHandoffErrorV2>
    {
        if let Err(transition) = self.revalidate_paused_v2() {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        let activation_deadline = self.foundation.startup_budget.operation_cutoff();
        let finalizer_generation = match self
            .foundation
            .activate_process_finalizer_until_v2(activation_deadline)
            .await
        {
            Ok(generation) => generation,
            Err(error) => {
                let transition = map_finalizer_activation_failure_v2(error);
                let cleanup = self.shutdown().await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        if !self.foundation.enter_process_cleanup_mode_v2() {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::ProcessCleanupMode,
                cleanup,
            ));
        }
        if production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1()).is_some()
        {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
                cleanup,
            ));
        }
        if let Err(error) = self
            .lifecycle
            .activate_process_owner_in_place_v2(self.process_generation)
            .await
        {
            let transition = map_fixed_point_handoff_failure_v2(error);
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        let Self {
            discord,
            foundation,
            lifecycle,
            finalizer_handoff,
            process_generation,
        } = self;
        let _ = finalizer_handoff;
        let lifecycle = match lifecycle.try_into_process_frozen_v2() {
            Ok(lifecycle) => lifecycle,
            Err(lifecycle) => {
                let cleanup = shutdown_transferred_production_handoff_v2(
                    foundation,
                    RuntimeTransferredDiscordSupervisorV2::Process(discord),
                    *lifecycle,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    cleanup,
                ));
            }
        };
        let process = RuntimeProcessBoundProductionHandoffProcessV2 {
            discord,
            foundation,
            lifecycle,
            finalizer_generation,
            process_generation,
        };
        if let Err(transition) = process.revalidate_paused_v2() {
            let cleanup = process.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        Ok(process)
    }
}

impl RuntimeProcessBoundProductionHandoffProcessV2 {
    pub(crate) fn revalidate_paused_v2(
        &self,
    ) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate);
        }
        let finalizer_health = self
            .foundation
            .process_finalizer_health_v2()
            .ok_or(RuntimeProcessProductionHandoffFailureV2::FinalizerUnavailable)?;
        if !finalizer_health.is_ready() {
            return Err(RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal);
        }
        if self.lifecycle.process_generation_v2() != self.process_generation {
            return Err(RuntimeProcessProductionHandoffFailureV2::Owner);
        }
        self.lifecycle
            .revalidate_paused_v2()
            .map_err(map_fixed_point_handoff_failure_v2)?;
        production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        shutdown_process_bound_production_handoff_v2(
            self.foundation,
            self.discord,
            self.lifecycle,
            self.process_generation,
        )
        .await
    }

    pub(crate) async fn into_recovery_resume_v2(
        self,
    ) -> Result<RuntimeRecoveryResumeProcessV2, RuntimeProcessProductionHandoffErrorV2> {
        if let Err(transition) = self.revalidate_paused_v2() {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        let Self {
            discord,
            foundation,
            lifecycle,
            finalizer_generation,
            process_generation,
        } = self;
        let lifecycle = match lifecycle
            .into_production_handoff_v2(finalizer_generation)
            .await
        {
            Ok(lifecycle) => lifecycle,
            Err(failure) => {
                let transition = map_worker_production_handoff_failure_v2(failure.error_v2());
                let cleanup = shutdown_process_bound_production_handoff_v2(
                    foundation,
                    discord,
                    failure.into_state_v2(),
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let process = RuntimeRecoveryResumeProcessV2 {
            discord,
            foundation,
            lifecycle,
            process_generation,
        };
        if let Err(transition) = process.revalidate_paused_v2() {
            let cleanup = process.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        Ok(process)
    }
}

impl RuntimeRecoveryResumeProcessV2 {
    pub(crate) fn revalidate_paused_v2(
        &self,
    ) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate);
        }
        if !self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready())
        {
            return Err(RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal);
        }
        self.lifecycle
            .revalidate_paused_v2()
            .map_err(map_worker_production_handoff_failure_v2)?;
        production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        shutdown_recovery_resume_process_v2(
            self.foundation,
            self.discord,
            self.lifecycle,
            self.process_generation,
        )
        .await
    }

    pub(crate) async fn resume_recovery_v2(
        mut self,
    ) -> Result<RuntimeAdmissionAcknowledgingProcessV2, RuntimeProcessProductionHandoffErrorV2>
    {
        if let Err(transition) = self.revalidate_paused_v2() {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        let pre_gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        if !maintenance_gate_is_closed_v2(pre_gate) {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::MaintenanceGate,
                cleanup,
            ));
        }
        let pre_database =
            match collect_recovery_resume_database_evidence_v2(&self.foundation).await {
                Ok(evidence) => evidence,
                Err(transition) => {
                    let cleanup = self.shutdown().await;
                    return Err(finish_production_handoff_transition_v2(transition, cleanup));
                }
            };
        if production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1()).is_some()
        {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
                cleanup,
            ));
        }
        let coordinator_generation = self.lifecycle.coordinator_generation_v2();
        let pause = match self.lifecycle.observe_exact_pause_reservation_v2() {
            Ok(pause) => pause,
            Err(_) => {
                let cleanup = self.shutdown().await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::Gateway,
                    cleanup,
                ));
            }
        };
        let owner_receipt = self
            .lifecycle
            .recovery_resume_permit_v2()
            .owner_receipt()
            .clone();
        let resume_deadline = Instant::now() + Duration::from_secs(2);
        let evidence = match self
            .discord
            .resume_reserved_admission_in_place_v2(coordinator_generation, pause, resume_deadline)
            .await
        {
            RuntimeDiscordRecoveryResumeAttemptV2::Applied(evidence) => evidence,
            RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(_) => {
                let cleanup = self.shutdown().await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::DiscordNotApplied,
                    cleanup,
                ));
            }
            RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(_) => {
                let cleanup = self.shutdown().await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate,
                    cleanup,
                ));
            }
        };
        if evidence.coordinator_generation_v2() != coordinator_generation
            || evidence.expected_v2() != pause
        {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                cleanup,
            ));
        }
        let gateway_ready = match self.lifecycle.observe_exact_resumed_ready_attestation_v2() {
            Ok(ready) => ready,
            Err(_) => {
                let cleanup = self.shutdown().await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::Gateway,
                    cleanup,
                ));
            }
        };
        self.foundation
            .lifecycle_timing_v2()
            .record_exact_ready_v2();
        let post_database =
            match collect_recovery_resume_database_evidence_v2(&self.foundation).await {
                Ok(evidence) => evidence,
                Err(transition) => {
                    let cleanup = self.shutdown().await;
                    return Err(finish_production_handoff_transition_v2(transition, cleanup));
                }
            };
        let post_gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        if post_gate != pre_gate
            || !maintenance_gate_is_closed_v2(post_gate)
            || post_database.writer_fence_generation != pre_database.writer_fence_generation
            || !self
                .foundation
                .process_finalizer_health_v2()
                .is_some_and(|health| health.is_ready())
            || production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                .is_some()
        {
            let cleanup = self.shutdown().await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                cleanup,
            ));
        }
        let observation = RuntimeClosedRecoveryResumeObservationV2 {
            owner_receipt,
            readiness: post_database.readiness,
            gateway_ready,
            writer_fence_generation: post_database.writer_fence_generation,
            maintenance_gate_generation: post_gate.generation(),
        };
        let Self {
            discord,
            mut foundation,
            lifecycle,
            process_generation,
        } = self;
        let lifecycle = match lifecycle.into_admission_acknowledging_v2(observation).await {
            Ok(lifecycle) => lifecycle,
            Err(failure) => {
                let transition = map_worker_production_handoff_failure_v2(failure.error_v2());
                let cleanup = shutdown_recovery_resume_process_v2(
                    foundation,
                    discord,
                    failure.into_state_v2(),
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let Some(ingress_acknowledgement) = foundation.take_ingress_acknowledgement_supervisor_v2()
        else {
            let cleanup = shutdown_admission_acknowledging_process_without_lane_v2(
                foundation,
                discord,
                lifecycle,
                process_generation,
            )
            .await;
            return Err(finish_production_handoff_transition_v2(
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                cleanup,
            ));
        };
        let process = RuntimeAdmissionAcknowledgingProcessV2 {
            discord,
            foundation,
            lifecycle,
            ingress_acknowledgement,
            process_generation,
        };
        if let Err(transition) = process.revalidate_v2() {
            let cleanup = process.shutdown().await;
            return Err(finish_production_handoff_transition_v2(transition, cleanup));
        }
        Ok(process)
    }
}

struct RuntimeRecoveryResumeDatabaseEvidenceV2 {
    readiness: automation_runtime_worker::RuntimeCapabilityReadinessSetV2,
    writer_fence_generation: automation_runtime_controller::RuntimeWriterFenceGenerationV1,
}

async fn collect_recovery_resume_database_evidence_v2(
    foundation: &RuntimeProcessFoundationV1,
) -> Result<RuntimeRecoveryResumeDatabaseEvidenceV2, RuntimeProcessProductionHandoffFailureV2> {
    let cutoff = Instant::now() + Duration::from_secs(5);
    let readiness = foundation
        .databases
        .verify_readiness_refresh_until_v2(cutoff)
        .await
        .map_err(|_| RuntimeProcessProductionHandoffFailureV2::Database)?
        .into_exact_capability_receipts();
    let writer = foundation
        .databases
        .execution()
        .observe_writer_fence(RuntimeObserveWriterFenceV1)
        .await
        .map_err(|_| RuntimeProcessProductionHandoffFailureV2::Database)?;
    let RuntimeWriterFenceObservationV1::Open { generation, .. } = writer else {
        return Err(RuntimeProcessProductionHandoffFailureV2::Database);
    };
    Ok(RuntimeRecoveryResumeDatabaseEvidenceV2 {
        readiness,
        writer_fence_generation: generation,
    })
}

fn maintenance_gate_is_closed_v2(snapshot: RuntimeMaintenanceIngressGateSnapshotV2) -> bool {
    snapshot.stage() == RuntimeMaintenanceIngressGateStageV2::Closed
        && snapshot.active_permit_count() == 0
        && !snapshot.shutdown_sealed()
        && snapshot.terminal_error().is_none()
}

impl RuntimeAdmissionAcknowledgingProcessV2 {
    pub(crate) fn revalidate_v2(&self) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate);
        }
        if self.lifecycle.process_generation_v2() != self.process_generation {
            return Err(RuntimeProcessProductionHandoffFailureV2::Owner);
        }
        if !self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready())
        {
            return Err(RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal);
        }
        self.lifecycle
            .revalidate_v2()
            .map_err(map_worker_production_handoff_failure_v2)?;
        production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        shutdown_admission_acknowledging_process_v2(
            self.foundation,
            self.discord,
            self.lifecycle,
            self.ingress_acknowledgement,
            self.process_generation,
        )
        .await
    }

    pub(crate) async fn enter_empty_open_v2(
        mut self,
    ) -> Result<RuntimeEmptyOpenProcessV2, RuntimeProcessProductionHandoffErrorV2> {
        if let Err(transition) = self.revalidate_v2() {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let Some(readiness) = self.foundation.take_readiness_publisher_v2() else {
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)
                .await);
        };
        let Some(controller) = self.foundation.maintenance_ingress_controller_v2() else {
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::MaintenanceGate)
                .await);
        };
        let opening = match controller.begin_open_v2() {
            Ok(opening) => opening,
            Err(failure) => {
                drop(failure.into_state());
                return Err(self
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::MaintenanceGate,
                    )
                    .await);
            }
        };
        let maintenance_ingress = match opening.commit_open_v2() {
            Ok(open) => open,
            Err(failure) => {
                drop(failure.into_state());
                return Err(self
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::MaintenanceGate,
                    )
                    .await);
            }
        };
        let open_generation = maintenance_ingress.generation();
        let predecessor_authorization = self
            .lifecycle
            .authorize_ingress_acknowledgement_predecessor_observation_v2();
        let predecessor_observation = match self
            .foundation
            .databases
            .execution()
            .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
            .await
        {
            Ok(observation) => observation,
            Err(_) => {
                drop(maintenance_ingress);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Database)
                    .await);
            }
        };
        let predecessor = match predecessor_authorization.accept(predecessor_observation) {
            Ok(predecessor) => predecessor,
            Err(_) => {
                drop(maintenance_ingress);
                return Err(self
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    )
                    .await);
            }
        };
        let lease_for = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(
            INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
        )
        .expect("bounded ingress acknowledgement lease");
        let Self {
            discord,
            foundation,
            lifecycle,
            mut ingress_acknowledgement,
            process_generation,
        } = self;
        let authority = match lifecycle.into_ingress_acknowledgement_authority_v2(
            open_generation,
            predecessor,
            lease_for,
        ) {
            Ok(authority) => authority,
            Err(failure) => {
                let transition =
                    map_ingress_acknowledgement_authorization_failure_v2(failure.error_v2());
                let process = Self {
                    discord,
                    foundation,
                    lifecycle: failure.into_state_v2(),
                    ingress_acknowledgement,
                    process_generation,
                };
                drop(maintenance_ingress);
                return Err(process.cleanup_transition_v2(transition).await);
            }
        };
        let acknowledgement = execute_ingress_acknowledgement_v2(
            &mut ingress_acknowledgement,
            authority,
            Instant::now() + Duration::from_secs(5),
            foundation.lifecycle_timing_v2(),
        )
        .await;
        let (lifecycle, accepted) = match acknowledgement {
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::Admission {
                lifecycle,
                accepted,
            }) => (*lifecycle, *accepted),
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::EmptyOpenRefresh {
                lifecycle,
                ..
            }) => {
                let cleanup = shutdown_orphaned_empty_open_process_v2(
                    foundation,
                    discord,
                    *lifecycle,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(ingress_acknowledgement, None),
                    maintenance_ingress,
                    readiness,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    cleanup,
                ));
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                authority,
                transition,
            }) => {
                let retained = (*authority).into_retained_state_v2();
                let RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::Admission(
                    lifecycle,
                ) = retained
                else {
                    let cleanup = shutdown_orphaned_ingress_acknowledgement_v2(
                        foundation,
                        discord,
                        retained,
                        RuntimeIngressAcknowledgementCleanupV2::new_v2(
                            ingress_acknowledgement,
                            None,
                        ),
                        maintenance_ingress,
                        readiness,
                        process_generation,
                    )
                    .await;
                    return Err(finish_production_handoff_transition_v2(transition, cleanup));
                };
                let process = Self {
                    discord,
                    foundation,
                    lifecycle: *lifecycle,
                    ingress_acknowledgement,
                    process_generation,
                };
                drop(maintenance_ingress);
                return Err(process.cleanup_transition_v2(transition).await);
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::AuthorityLost(
                transition,
            )) => {
                drop(maintenance_ingress);
                let cleanup = shutdown_process_without_lifecycle_v2(
                    foundation,
                    discord,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(ingress_acknowledgement, None),
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        self = Self {
            discord,
            foundation,
            lifecycle,
            ingress_acknowledgement,
            process_generation,
        };
        let acknowledgement_database_now = accepted.receipt().observed_database_now();
        let acknowledged_owner = accepted.request().owner_receipt().clone();
        let mut current_owner = match self.lifecycle.observe_current_owner_v2().await {
            Ok(owner) => owner,
            Err(_) => {
                drop(maintenance_ingress);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                    .await);
            }
        };
        if current_owner.lease_id != acknowledged_owner.lease_id
            || current_owner.owner_revision != acknowledged_owner.owner_revision
            || current_owner.expires_at != acknowledged_owner.expires_at
            || acknowledgement_database_now < current_owner.database_now
            || acknowledgement_database_now >= current_owner.expires_at
        {
            drop(maintenance_ingress);
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                .await);
        }
        current_owner.database_now = acknowledgement_database_now;
        let database = match collect_recovery_resume_database_evidence_v2(&self.foundation).await {
            Ok(database) => database,
            Err(transition) => {
                drop(maintenance_ingress);
                return Err(self.cleanup_transition_v2(transition).await);
            }
        };
        let gateway_ready = match self.lifecycle.observe_exact_current_ready_attestation_v2() {
            Ok(ready) => ready,
            Err(_) => {
                drop(maintenance_ingress);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        let registry_empty = match self.lifecycle.observe_registry_empty_v2() {
            Ok(empty) => empty,
            Err(_) => {
                drop(maintenance_ingress);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Registry)
                    .await);
            }
        };
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        let finalizer_accepting = self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready());
        if !maintenance_gate_is_open_v2(gate, open_generation)
            || !finalizer_accepting
            || self.discord.terminal_status_v2().is_some()
            || self.discord.is_finished_v2()
            || production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                .is_some()
        {
            drop(maintenance_ingress);
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)
                .await);
        }
        let final_acknowledgement_observation_started_at = Instant::now();
        let final_acknowledgement = match exact_reobserve_ingress_acknowledgement_v2(
            self.foundation.databases.execution(),
            self.lifecycle
                .authorize_ingress_acknowledgement_predecessor_observation_v2(),
            accepted.receipt(),
        )
        .await
        {
            Ok(database_now) => database_now,
            Err(transition) => {
                drop(maintenance_ingress);
                return Err(self.cleanup_transition_v2(transition).await);
            }
        };
        let final_acknowledgement_database_now = final_acknowledgement.observed_database_now();
        if final_acknowledgement_database_now < current_owner.database_now
            || final_acknowledgement_database_now >= current_owner.expires_at
        {
            drop(maintenance_ingress);
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                .await);
        }
        current_owner.database_now = final_acknowledgement_database_now;
        let acknowledged_owner_revision = current_owner.owner_revision;
        let acknowledgement_schedule = match ingress_acknowledgement_schedule_v2(
            &final_acknowledgement,
            final_acknowledgement_observation_started_at,
        ) {
            Some(schedule) => schedule,
            None => {
                drop(maintenance_ingress);
                return Err(self
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    )
                    .await);
            }
        };
        let finalizer_generation = self.lifecycle.finalizer_generation_v2();
        let observation = RuntimeOpenProductionObservationInputV2 {
            coordinator_generation: self.lifecycle.coordinator_generation_v2(),
            writer_fence_generation: database.writer_fence_generation,
            writer_fence_open: true,
            maintenance_gate_generation: gate.generation(),
            maintenance_gate_open: true,
            owner_receipt: current_owner,
            readiness: database.readiness,
            gateway_ready,
            registry_empty,
            finalizer_generation,
            finalizer_accepting,
            supervisors_running: true,
            observed_database_now: final_acknowledgement_database_now,
            ingress_acknowledgement:
                automation_runtime_worker::RuntimeIngressOpenAcknowledgementObservationV2::from_accepted(
                    accepted,
                ),
        };
        let mut lifecycle_slot = Some(self.lifecycle);
        let lifecycle_transition = maintenance_ingress.linearize_open_transition_v2(|| {
            lifecycle_slot
                .take()
                .expect("admission acknowledgement lifecycle")
                .into_empty_open_v2(observation)
        });
        let lifecycle = match lifecycle_transition {
            Ok(Ok(lifecycle)) => lifecycle,
            Ok(Err(failure)) => {
                let transition = map_worker_production_handoff_failure_v2(failure.error_v2());
                let process = Self {
                    discord: self.discord,
                    foundation: self.foundation,
                    lifecycle: failure.into_state_v2(),
                    ingress_acknowledgement: self.ingress_acknowledgement,
                    process_generation: self.process_generation,
                };
                drop(maintenance_ingress);
                return Err(process.cleanup_transition_v2(transition).await);
            }
            Err(_) => {
                let process = Self {
                    discord: self.discord,
                    foundation: self.foundation,
                    lifecycle: lifecycle_slot
                        .take()
                        .expect("rejected admission acknowledgement lifecycle"),
                    ingress_acknowledgement: self.ingress_acknowledgement,
                    process_generation: self.process_generation,
                };
                drop(maintenance_ingress);
                return Err(process
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
                    )
                    .await);
            }
        };
        let lifecycle = match lifecycle.start_production_owner_v2().await {
            Ok(lifecycle) => lifecycle,
            Err(failure) => {
                let transition = match failure.error_v2() {
                    crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerProcessRenewalStartErrorV2::OwnerReceiptMismatch => RuntimeProcessProductionHandoffFailureV2::Owner,
                    crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProcessGenerationMismatch
                    | crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation
                    | crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerProcessRenewalStartErrorV2::SupervisorUnavailable => RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                };
                let cleanup = shutdown_frozen_empty_open_process_v2(
                    self.foundation,
                    self.discord,
                    failure.into_state_v2(),
                    maintenance_ingress,
                    readiness,
                    self.ingress_acknowledgement,
                    self.process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let acknowledgement_safety = RuntimeIngressAcknowledgementSafetyMonitorV2::start_v2(
            acknowledgement_schedule.safety_deadline,
            self.foundation.invalidation_trigger_v1(),
        );
        let successor = match lifecycle
            .wait_for_owner_successor_v2(
                acknowledged_owner_revision,
                acknowledgement_schedule.safety_deadline,
            )
            .await
        {
            Ok(successor) => successor,
            Err(_) => {
                let process = RuntimeEmptyOpenProcessV2 {
                    discord: self.discord,
                    foundation: self.foundation,
                    lifecycle,
                    maintenance_ingress,
                    readiness,
                    ingress_acknowledgement: self.ingress_acknowledgement,
                    acknowledgement_schedule,
                    acknowledgement_safety,
                    process_generation: self.process_generation,
                };
                return Err(process
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                    .await);
            }
        };
        let process = RuntimeEmptyOpenProcessV2 {
            discord: self.discord,
            foundation: self.foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement: self.ingress_acknowledgement,
            acknowledgement_schedule,
            acknowledgement_safety,
            process_generation: self.process_generation,
        };
        let mut process = process
            .refresh_acknowledgement_with_owner_v2(successor.receipt().clone())
            .await?;
        if let Err(error) = process
            .foundation
            .activate_capability_readiness_supervisor_v2(
                process.acknowledgement_schedule.safety_deadline,
            )
            .await
        {
            return Err(process
                .cleanup_transition_v2(map_capability_readiness_activation_failure_v2(error))
                .await);
        }
        if !process
            .lifecycle
            .arm_gateway_invalidation_trigger_v2(process.foundation.invalidation_trigger_v1())
        {
            return Err(process
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                .await);
        }
        if let Err(transition) = process.revalidate_v2() {
            return Err(process.cleanup_transition_v2(transition).await);
        }
        if !process.readiness.publish_ready_v2() {
            return Err(process
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::ProcessShutdown)
                .await);
        }
        Ok(process)
    }

    async fn cleanup_transition_v2(
        self,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        let cleanup = self.shutdown().await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

enum RuntimeProcessIngressAcknowledgementExecutionFailureV2 {
    Retained {
        authority: Box<RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2>,
        transition: RuntimeProcessProductionHandoffFailureV2,
    },
    AuthorityLost(RuntimeProcessProductionHandoffFailureV2),
}

async fn execute_ingress_acknowledgement_v2(
    supervisor: &mut RuntimeProcessIngressAcknowledgementSupervisorV2,
    authority: RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2,
    deadline: Instant,
    lifecycle_timing: crate::lifecycle_timing::RuntimeLifecycleTimingRecorderV2,
) -> Result<
    RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2,
    RuntimeProcessIngressAcknowledgementExecutionFailureV2,
> {
    let job = RuntimeWorkerIngressAcknowledgementJobV2::new(authority);
    let waiter = match supervisor.try_submit(job, deadline) {
        Ok(waiter) => waiter,
        Err(rejection) => {
            lifecycle_timing.record_durable_acknowledgement_terminal_v2(
                crate::lifecycle_timing::RuntimeLifecycleTimingOutcomeV2::Rejected,
            );
            let transition =
                map_ingress_acknowledgement_registration_rejection_v2(rejection.reason());
            return Err(
                RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                    authority: Box::new(rejection.into_job().into_authority()),
                    transition,
                },
            );
        }
    };
    waiter.cancel_v2();
    let Some(completion) = supervisor.recv_completion().await else {
        lifecycle_timing.record_durable_acknowledgement_terminal_v2(
            crate::lifecycle_timing::RuntimeLifecycleTimingOutcomeV2::FailedClosed,
        );
        return Err(
            RuntimeProcessIngressAcknowledgementExecutionFailureV2::AuthorityLost(
                RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
            ),
        );
    };
    match completion.into_result() {
        RuntimeIngressAcknowledgementExecutionResultV2::Accepted(outcome) => {
            lifecycle_timing.record_durable_acknowledgement_terminal_v2(
                crate::lifecycle_timing::RuntimeLifecycleTimingOutcomeV2::Completed,
            );
            Ok(outcome)
        }
        RuntimeIngressAcknowledgementExecutionResultV2::CompletionRejected { job, error } => {
            lifecycle_timing.record_durable_acknowledgement_terminal_v2(
                crate::lifecycle_timing::RuntimeLifecycleTimingOutcomeV2::Rejected,
            );
            let transition = match error {
                crate::closed_recovery::RuntimeClosedRecoveryIngressAcknowledgementCompletionErrorV2::EmptyOpenRefresh(
                    lifecycle,
                ) => map_production_lifecycle_failure_v2(lifecycle),
            };
            Err(
                RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                    authority: Box::new(job.into_authority()),
                    transition,
                },
            )
        }
        RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed { job, failure } => {
            lifecycle_timing.record_durable_acknowledgement_terminal_v2(
                if matches!(
                    failure,
                    RuntimeIngressAcknowledgementFailureV2::OperationDeadlineElapsed
                ) {
                    crate::lifecycle_timing::RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
                } else {
                    crate::lifecycle_timing::RuntimeLifecycleTimingOutcomeV2::FailedClosed
                },
            );
            Err(
                RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                    authority: Box::new(job.into_authority()),
                    transition: map_ingress_acknowledgement_failure_v2(failure),
                },
            )
        }
    }
}

fn map_ingress_acknowledgement_registration_rejection_v2(
    rejection: RuntimeIngressAcknowledgementRegistrationRejectionReasonV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match rejection {
        RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::DeadlineElapsed => {
            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed
        }
        RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::IntakeSealed
        | RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::SupervisorTerminal(_) => {
            RuntimeProcessProductionHandoffFailureV2::ProcessShutdown
        }
        RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::Busy
        | RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::SequenceExhausted => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
    }
}

fn map_ingress_acknowledgement_failure_v2(
    failure: RuntimeIngressAcknowledgementFailureV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match failure {
        RuntimeIngressAcknowledgementFailureV2::OperationDeadlineElapsed => {
            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed
        }
        RuntimeIngressAcknowledgementFailureV2::Shutdown => {
            RuntimeProcessProductionHandoffFailureV2::ProcessShutdown
        }
        RuntimeIngressAcknowledgementFailureV2::ObservationAuthorityLost
        | RuntimeIngressAcknowledgementFailureV2::Stale
        | RuntimeIngressAcknowledgementFailureV2::Divergent => {
            RuntimeProcessProductionHandoffFailureV2::Database
        }
        RuntimeIngressAcknowledgementFailureV2::AttemptUnavailable
        | RuntimeIngressAcknowledgementFailureV2::ObservationProtocolViolation
        | RuntimeIngressAcknowledgementFailureV2::ReplayBudgetExhausted
        | RuntimeIngressAcknowledgementFailureV2::SecondUncertainty
        | RuntimeIngressAcknowledgementFailureV2::ResolutionProtocolViolation => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
    }
}

fn map_ingress_acknowledgement_authorization_failure_v2(
    error: automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match error {
        automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2::OpenGateMismatch
        | automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2::PreviousAcknowledgementMismatch
        | automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2::PredecessorObservationMismatch
        | automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2::InvalidRequest => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
    }
}

fn map_production_lifecycle_failure_v2(
    error: automation_runtime_worker::RuntimeProductionLifecycleErrorV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    use automation_runtime_worker::RuntimeProductionLifecycleErrorV2;

    match error {
        RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch
        | RuntimeProductionLifecycleErrorV2::StaleAdmissionRevision
        | RuntimeProductionLifecycleErrorV2::GatewayReadyMismatch
        | RuntimeProductionLifecycleErrorV2::ExplicitResumeMissing
        | RuntimeProductionLifecycleErrorV2::StaleGeneration => {
            RuntimeProcessProductionHandoffFailureV2::Gateway
        }
        RuntimeProductionLifecycleErrorV2::RegistryMismatch => {
            RuntimeProcessProductionHandoffFailureV2::Registry
        }
        RuntimeProductionLifecycleErrorV2::OwnerMismatch => {
            RuntimeProcessProductionHandoffFailureV2::Owner
        }
        RuntimeProductionLifecycleErrorV2::ReadinessMismatch
        | RuntimeProductionLifecycleErrorV2::WriterFenceMismatch
        | RuntimeProductionLifecycleErrorV2::IngressAcknowledgementNotCurrent => {
            RuntimeProcessProductionHandoffFailureV2::Database
        }
        RuntimeProductionLifecycleErrorV2::FinalizerGenerationMismatch => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal
        }
        RuntimeProductionLifecycleErrorV2::FixedPoint(_)
        | RuntimeProductionLifecycleErrorV2::HandoffEvidenceMismatch
        | RuntimeProductionLifecycleErrorV2::StartupIntakeNotSealed
        | RuntimeProductionLifecycleErrorV2::StartupJobsUnsettled
        | RuntimeProductionLifecycleErrorV2::SupervisorsNotReady
        | RuntimeProductionLifecycleErrorV2::ResumePermitMismatch
        | RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch
        | RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch
        | RuntimeProductionLifecycleErrorV2::SequenceOutOfRange
        | RuntimeProductionLifecycleErrorV2::GenerationOverflow => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
    }
}

async fn exact_reobserve_ingress_acknowledgement_v2<P>(
    port: &P,
    authorization:
        automation_runtime_worker::RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2,
    expected: &automation_runtime_controller::RuntimeIngressOpenAcknowledgementReceiptV2,
) -> Result<
    automation_runtime_controller::RuntimeIngressOpenAcknowledgementReceiptV2,
    RuntimeProcessProductionHandoffFailureV2,
>
where
    P: RuntimeIngressOpenAcknowledgementPortV2,
{
    let observation = port
        .observe_ingress_open_acknowledgement_predecessor(&authorization)
        .await
        .map_err(|_| RuntimeProcessProductionHandoffFailureV2::Database)?;
    let predecessor = authorization
        .accept(observation)
        .map_err(|_| RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)?;
    let current = predecessor
        .present_receipt()
        .ok_or(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)?;
    if current.source_acknowledgement_revision() != expected.source_acknowledgement_revision()
        || current.request_digest() != expected.request_digest()
        || current.acknowledgement() != expected.acknowledgement()
        || current.observed_database_now() < expected.observed_database_now()
        || current.observed_database_now() >= current.acknowledgement().expires_at()
    {
        return Err(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation);
    }
    Ok(current.clone())
}

fn ingress_acknowledgement_schedule_v2(
    receipt: &automation_runtime_controller::RuntimeIngressOpenAcknowledgementReceiptV2,
    observation_started_at: Instant,
) -> Option<RuntimeIngressAcknowledgementScheduleV2> {
    let remaining = receipt
        .acknowledgement()
        .expires_at()
        .signed_duration_since(receipt.observed_database_now())
        .to_std()
        .ok()?;
    ingress_acknowledgement_schedule_from_remaining_v2(
        remaining,
        observation_started_at,
        Instant::now(),
    )
}

fn ingress_acknowledgement_schedule_from_remaining_v2(
    remaining: Duration,
    observation_started_at: Instant,
    observed_at: Instant,
) -> Option<RuntimeIngressAcknowledgementScheduleV2> {
    let safety_remaining = remaining.checked_sub(INGRESS_ACKNOWLEDGEMENT_SAFETY_MARGIN_V2)?;
    if safety_remaining.is_zero() {
        return None;
    }
    let safety_deadline = observation_started_at.checked_add(safety_remaining)?;
    if observed_at >= safety_deadline {
        return None;
    }
    let refresh_remaining = remaining
        .checked_sub(INGRESS_ACKNOWLEDGEMENT_REFRESH_ADVANCE_V2)
        .unwrap_or(Duration::ZERO)
        .min(safety_remaining);
    let refresh_at = observation_started_at.checked_add(refresh_remaining)?;
    Some(RuntimeIngressAcknowledgementScheduleV2 {
        refresh_at,
        safety_deadline,
    })
}

fn maintenance_gate_is_open_v2(
    snapshot: RuntimeMaintenanceIngressGateSnapshotV2,
    expected_generation: automation_runtime_worker::RuntimeMaintenanceGateGenerationV2,
) -> bool {
    snapshot.generation() == expected_generation
        && snapshot.stage() == RuntimeMaintenanceIngressGateStageV2::Open
        && snapshot.active_permit_count() == 0
        && !snapshot.shutdown_sealed()
        && snapshot.terminal_error().is_none()
}

impl RuntimeEmptyOpenProcessV2 {
    pub(crate) fn revalidate_v2(&self) -> Result<(), RuntimeProcessProductionHandoffFailureV2> {
        if self.discord.terminal_status_v2().is_some() || self.discord.is_finished_v2() {
            return Err(RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate);
        }
        if self.lifecycle.process_generation_v2() != self.process_generation {
            return Err(RuntimeProcessProductionHandoffFailureV2::Owner);
        }
        if !self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready())
        {
            return Err(RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal);
        }
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        if !maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation()) {
            return Err(RuntimeProcessProductionHandoffFailureV2::MaintenanceGate);
        }
        self.lifecycle
            .revalidate_v2()
            .map_err(map_worker_production_handoff_failure_v2)?;
        production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
            .map_or(Ok(()), Err)
    }

    async fn refresh_acknowledgement_with_owner_v2(
        self,
        owner_receipt: automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
    ) -> Result<Self, RuntimeProcessProductionHandoffErrorV2> {
        if Instant::now() >= self.acknowledgement_schedule.safety_deadline {
            return Err(self
                .cleanup_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                )
                .await);
        }
        if let Err(transition) = self.revalidate_v2() {
            return Err(self.cleanup_transition_v2(transition).await);
        }
        let database = match collect_recovery_resume_database_evidence_v2(&self.foundation).await {
            Ok(database) => database,
            Err(transition) => return Err(self.cleanup_transition_v2(transition).await),
        };
        let gateway_ready = match self.lifecycle.observe_exact_current_ready_attestation_v2() {
            Ok(ready) => ready,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        let registry_empty = match self.lifecycle.observe_registry_empty_v2() {
            Ok(empty) => empty,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Registry)
                    .await);
            }
        };
        let gate = self
            .foundation
            .maintenance_ingress_observer_v2()
            .snapshot_v2();
        let finalizer_accepting = self
            .foundation
            .process_finalizer_health_v2()
            .is_some_and(|health| health.is_ready());
        let supervisors_running = finalizer_accepting
            && self.lifecycle.owner_terminal_status_v2().is_none()
            && self.discord.terminal_status_v2().is_none()
            && !self.discord.is_finished_v2()
            && production_handoff_shutdown_failure_v2(&self.foundation.shutdown_observer_v1())
                .is_none();
        if !maintenance_gate_is_open_v2(gate, self.maintenance_ingress.generation())
            || !supervisors_running
        {
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::ProtocolViolation)
                .await);
        }
        let predecessor_authorization = self
            .lifecycle
            .authorize_ingress_acknowledgement_predecessor_observation_v2();
        let final_authorization = self
            .lifecycle
            .authorize_ingress_acknowledgement_predecessor_observation_v2();
        let predecessor_observation = match self
            .foundation
            .databases
            .execution()
            .observe_ingress_open_acknowledgement_predecessor(&predecessor_authorization)
            .await
        {
            Ok(observation) => observation,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Database)
                    .await);
            }
        };
        let predecessor = match predecessor_authorization.accept(predecessor_observation) {
            Ok(predecessor) => predecessor,
            Err(_) => {
                return Err(self
                    .cleanup_transition_v2(
                        RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    )
                    .await);
            }
        };
        let lease_for = RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(
            INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
        )
        .expect("bounded ingress acknowledgement lease");
        let input = RuntimeEmptyOpenAcknowledgementRefreshInputV2 {
            owner_receipt,
            readiness: database.readiness,
            gateway_ready,
            writer_fence_generation: database.writer_fence_generation,
            writer_fence_open: true,
            maintenance_gate_generation: gate.generation(),
            maintenance_gate_open: true,
            registry_empty,
            finalizer_generation: self.lifecycle.finalizer_generation_v2(),
            finalizer_accepting,
            supervisors_running,
            predecessor,
            lease_for,
        };
        let Self {
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            mut ingress_acknowledgement,
            acknowledgement_schedule,
            acknowledgement_safety,
            process_generation,
        } = self;
        let refresh = match lifecycle.authorize_acknowledgement_refresh_v2(input) {
            Ok(refresh) => refresh,
            Err(failure) => {
                let transition = map_production_lifecycle_failure_v2(failure.error_v2());
                let process = Self {
                    discord,
                    foundation,
                    lifecycle: failure.into_state_v2(),
                    maintenance_ingress,
                    readiness,
                    ingress_acknowledgement,
                    acknowledgement_schedule,
                    acknowledgement_safety,
                    process_generation,
                };
                return Err(process.cleanup_transition_v2(transition).await);
            }
        };
        let authority = refresh.into_ingress_acknowledgement_authority_v2();
        let acknowledgement = execute_ingress_acknowledgement_v2(
            &mut ingress_acknowledgement,
            authority,
            acknowledgement_schedule.safety_deadline,
            foundation.lifecycle_timing_v2(),
        )
        .await;
        let (lifecycle, accepted_receipt) = match acknowledgement {
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::EmptyOpenRefresh {
                lifecycle,
                accepted_receipt,
            }) => (*lifecycle, *accepted_receipt),
            Ok(RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::Admission {
                lifecycle,
                ..
            }) => {
                let cleanup = shutdown_orphaned_admission_process_v2(
                    foundation,
                    discord,
                    *lifecycle,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(
                        ingress_acknowledgement,
                        Some(acknowledgement_safety),
                    ),
                    maintenance_ingress,
                    readiness,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProtocolViolation,
                    cleanup,
                ));
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::Retained {
                authority,
                transition,
            }) => {
                let retained = (*authority).into_retained_state_v2();
                let RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::EmptyOpenRefresh(
                    refresh,
                ) = retained
                else {
                    let cleanup = shutdown_orphaned_ingress_acknowledgement_v2(
                        foundation,
                        discord,
                        retained,
                        RuntimeIngressAcknowledgementCleanupV2::new_v2(
                            ingress_acknowledgement,
                            Some(acknowledgement_safety),
                        ),
                        maintenance_ingress,
                        readiness,
                        process_generation,
                    )
                    .await;
                    return Err(finish_production_handoff_transition_v2(transition, cleanup));
                };
                let cleanup = shutdown_refreshing_empty_open_process_v2(
                    foundation,
                    discord,
                    *refresh,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(
                        ingress_acknowledgement,
                        Some(acknowledgement_safety),
                    ),
                    maintenance_ingress,
                    readiness,
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
            Err(RuntimeProcessIngressAcknowledgementExecutionFailureV2::AuthorityLost(
                transition,
            )) => {
                let cleanup = shutdown_process_without_lifecycle_v2(
                    foundation,
                    discord,
                    RuntimeIngressAcknowledgementCleanupV2::new_v2(
                        ingress_acknowledgement,
                        Some(acknowledgement_safety),
                    ),
                    process_generation,
                )
                .await;
                return Err(finish_production_handoff_transition_v2(transition, cleanup));
            }
        };
        let final_observation_started_at = Instant::now();
        let final_observation = exact_reobserve_ingress_acknowledgement_v2(
            foundation.databases.execution(),
            final_authorization,
            &accepted_receipt,
        )
        .await;
        let schedule = final_observation.as_ref().ok().and_then(|receipt| {
            ingress_acknowledgement_schedule_v2(receipt, final_observation_started_at)
        });
        let process = Self {
            discord,
            foundation,
            lifecycle,
            maintenance_ingress,
            readiness,
            ingress_acknowledgement,
            acknowledgement_schedule: schedule.unwrap_or(acknowledgement_schedule),
            acknowledgement_safety,
            process_generation,
        };
        match (final_observation, schedule) {
            (Ok(_), Some(schedule)) => {
                if !process
                    .acknowledgement_safety
                    .rearm_v2(schedule.safety_deadline)
                {
                    Err(process
                        .cleanup_transition_v2(
                            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                        )
                        .await)
                } else if let Err(transition) = process.revalidate_v2() {
                    Err(process.cleanup_transition_v2(transition).await)
                } else {
                    Ok(process)
                }
            }
            (Err(transition), _) => Err(process.cleanup_transition_v2(transition).await),
            (Ok(_), None) => Err(process
                .cleanup_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed,
                )
                .await),
        }
    }

    pub(crate) async fn run_until_shutdown_v2(
        mut self,
    ) -> Result<(), RuntimeProcessProductionHandoffErrorV2> {
        let gateway_ready = match self.lifecycle.observe_exact_current_ready_attestation_v2() {
            Ok(gateway_ready) => gateway_ready,
            Err(_) => {
                self.foundation
                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
                return Err(self
                    .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                    .await);
            }
        };
        let gateway_invalidation = self
            .lifecycle
            .bind_gateway_ready_invalidation_observer_v2(&gateway_ready);
        if gateway_invalidation.current_invalidation_v2().is_some() {
            self.foundation
                .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
            return Err(self
                .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Gateway)
                .await);
        }
        let trigger = self.foundation.shutdown_trigger_v1();
        let mut shutdown_for_monitor = self.foundation.shutdown_observer_v1();
        let mut discord_terminal = self.discord.observation_v2();
        let owner_terminal = self.lifecycle.owner_terminal_observation_v2();
        let monitor = RuntimeEmptyOpenMonitorV2::start(async move {
            tokio::select! {
                biased;
                _ = shutdown_for_monitor.wait() => {}
                _ = gateway_invalidation.wait_v2() => {
                    trigger.trip(crate::RuntimeShutdownCauseV1::ReadinessLost);
                }
                _ = discord_terminal.wait_terminal() => {
                    trigger.trip(crate::RuntimeShutdownCauseV1::DiscordTerminal);
                }
                _ = owner_terminal => {
                    trigger.trip(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
                }
            }
        });
        let mut shutdown = self.foundation.shutdown_observer_v1();
        let observation = loop {
            if let Some(observation) = shutdown.observed() {
                break observation;
            }
            if Instant::now() >= self.acknowledgement_schedule.safety_deadline {
                break self
                    .foundation
                    .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::ReadinessLost);
            }
            let refresh_at = self.acknowledgement_schedule.refresh_at;
            let selected = tokio::select! {
                biased;
                observation = shutdown.wait() => Some(observation),
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(refresh_at)) => None,
            };
            if let Some(observation) = selected {
                break observation;
            }
            let owner = match self.lifecycle.observe_current_owner_v2().await {
                Ok(owner) => owner,
                Err(_) => {
                    self.foundation
                        .trip_shutdown_v1(crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal);
                    monitor.stop_v2().await;
                    return Err(self
                        .cleanup_transition_v2(RuntimeProcessProductionHandoffFailureV2::Owner)
                        .await);
                }
            };
            match self
                .refresh_acknowledgement_with_owner_v2(owner.receipt().clone())
                .await
            {
                Ok(process) => self = process,
                Err(error) => {
                    monitor.stop_v2().await;
                    return Err(error);
                }
            }
        };
        monitor.stop_v2().await;
        let cleanup = self.shutdown().await;
        match observation.cause() {
            crate::RuntimeShutdownCauseV1::Interrupt
            | crate::RuntimeShutdownCauseV1::Terminate
            | crate::RuntimeShutdownCauseV1::Explicit => cleanup.map_err(|cleanup| {
                finish_production_handoff_transition_v2(
                    RuntimeProcessProductionHandoffFailureV2::ProcessShutdown,
                    Err(cleanup),
                )
            }),
            cause => Err(finish_production_handoff_transition_v2(
                map_empty_open_shutdown_cause_v2(cause),
                cleanup,
            )),
        }
    }

    pub(crate) async fn shutdown(self) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
        let _revalidation = self.revalidate_v2();
        shutdown_empty_open_process_v2(
            self.foundation,
            self.discord,
            self.lifecycle,
            RuntimeIngressAcknowledgementCleanupV2::new_v2(
                self.ingress_acknowledgement,
                Some(self.acknowledgement_safety),
            ),
            self.maintenance_ingress,
            self.readiness,
            self.process_generation,
        )
        .await
    }

    async fn cleanup_transition_v2(
        self,
        transition: RuntimeProcessProductionHandoffFailureV2,
    ) -> RuntimeProcessProductionHandoffErrorV2 {
        let cleanup = self.shutdown().await;
        finish_production_handoff_transition_v2(transition, cleanup)
    }
}

fn map_capability_readiness_activation_failure_v2(
    failure: crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match failure {
        crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::DeadlineElapsed => {
            RuntimeProcessProductionHandoffFailureV2::OperationDeadlineElapsed
        }
        crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::Sealed => {
            RuntimeProcessProductionHandoffFailureV2::ProcessShutdown
        }
        crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::ReadinessUnavailable
        | crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::ReadinessTimedOut => {
            RuntimeProcessProductionHandoffFailureV2::Database
        }
        crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::AlreadyActivated
        | crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::ControlClosed
        | crate::capability_readiness_supervisor::RuntimeCapabilityReadinessActivationErrorV2::ResponseLost => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
    }
}

struct RuntimeEmptyOpenMonitorV2 {
    task: Option<tokio::task::JoinHandle<()>>,
}

impl RuntimeEmptyOpenMonitorV2 {
    fn start(task: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            task: Some(tokio::spawn(task)),
        }
    }

    async fn stop_v2(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _result = task.await;
        }
    }
}

impl Drop for RuntimeEmptyOpenMonitorV2 {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

fn map_empty_open_shutdown_cause_v2(
    cause: crate::RuntimeShutdownCauseV1,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match cause {
        crate::RuntimeShutdownCauseV1::GatewayOwnerTerminal => {
            RuntimeProcessProductionHandoffFailureV2::Owner
        }
        crate::RuntimeShutdownCauseV1::DiscordTerminal => {
            RuntimeProcessProductionHandoffFailureV2::DiscordIndeterminate
        }
        crate::RuntimeShutdownCauseV1::FinalizerTerminal => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerTerminal
        }
        crate::RuntimeShutdownCauseV1::ReadinessLost => {
            RuntimeProcessProductionHandoffFailureV2::Database
        }
        crate::RuntimeShutdownCauseV1::HealthTerminal
        | crate::RuntimeShutdownCauseV1::IngressAcknowledgementTerminal
        | crate::RuntimeShutdownCauseV1::SupervisorFailure => {
            RuntimeProcessProductionHandoffFailureV2::ProtocolViolation
        }
        crate::RuntimeShutdownCauseV1::Interrupt
        | crate::RuntimeShutdownCauseV1::Terminate
        | crate::RuntimeShutdownCauseV1::Explicit => {
            RuntimeProcessProductionHandoffFailureV2::ProcessShutdown
        }
    }
}

fn production_handoff_shutdown_failure_v2(
    shutdown: &crate::shutdown::RuntimeShutdownObserverV1,
) -> Option<RuntimeProcessProductionHandoffFailureV2> {
    shutdown
        .observed()
        .map(|_| RuntimeProcessProductionHandoffFailureV2::ProcessShutdown)
}

async fn time_shutdown_result_v2<T, E, F, Outcome>(
    timing: &RuntimeLifecycleTimingRecorderV2,
    metric: RuntimeLifecycleTimingMetricV2,
    future: F,
    outcome: Outcome,
) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
    Outcome: FnOnce(&Result<T, E>) -> RuntimeLifecycleTimingOutcomeV2,
{
    let span = timing.start_span_v2(metric);
    let result = future.await;
    span.finish_v2(outcome(&result));
    result
}

fn finish_observation_shutdown_timing_v2<T, E>(
    terminal: RuntimeLifecycleTimingTerminalReporterV2,
    result: Result<T, E>,
) -> Result<T, E> {
    let outcome = if result.is_ok() {
        RuntimeLifecycleTimingOutcomeV2::Completed
    } else {
        RuntimeLifecycleTimingOutcomeV2::FailedClosed
    };
    terminal.finish_result_v2(result, outcome)
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
    let (cleanup_deadline, terminal) = foundation
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
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until_v2(discord_drain, process_generation, discord_cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.abort_and_shutdown_until_v2(owner_cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_process_bound_production_handoff_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoveryProcessFrozenProcessV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.abort_and_shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_recovery_resume_process_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoveryProductionHandoffProcessV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    shutdown_recovery_resume_transferred_v2(
        foundation,
        RuntimeTransferredDiscordSupervisorV2::Process(discord),
        lifecycle,
        process_generation,
    )
    .await
}

async fn shutdown_recovery_resume_transferred_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeTransferredDiscordSupervisorV2,
    lifecycle: RuntimeClosedRecoveryProductionHandoffProcessV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until_v2(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.abort_and_shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_admission_acknowledging_process_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    shutdown_ingress_acknowledgement_supervisor_v2(
        &mut foundation,
        ingress_acknowledgement,
        cleanup_deadline,
    )
    .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.abort_and_shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_admission_acknowledging_process_without_lane_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.abort_and_shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_ingress_acknowledgement_supervisor_v2(
    foundation: &mut RuntimeProcessFoundationV1,
    ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    cleanup_deadline: Instant,
) {
    let timing = foundation
        .lifecycle_timing_v2()
        .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownIngressAcknowledgementJoin);
    let report = ingress_acknowledgement
        .shutdown_until(cleanup_deadline)
        .await;
    let outcome = if report.completion().is_some() {
        RuntimeLifecycleTimingOutcomeV2::FailedClosed
    } else {
        match report.exit() {
            crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementSupervisorExitV2::Commanded => RuntimeLifecycleTimingOutcomeV2::Completed,
            crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementSupervisorExitV2::DeadlineElapsed => RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed,
            crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementSupervisorExitV2::IntakeClosed
            | crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation
            | crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementSupervisorExitV2::Panicked
            | crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementSupervisorExitV2::Aborted => RuntimeLifecycleTimingOutcomeV2::FailedClosed,
        }
    };
    foundation
        .record_ingress_acknowledgement_shutdown_v2(report.exit(), report.completion().is_some());
    drop(report);
    timing.finish_v2(outcome);
}

async fn shutdown_orphaned_empty_open_process_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementCleanupV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    shutdown_empty_open_process_v2(
        foundation,
        discord,
        lifecycle,
        ingress_acknowledgement,
        maintenance_ingress,
        readiness,
        process_generation,
    )
    .await
}

async fn shutdown_orphaned_admission_process_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementCleanupV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let RuntimeIngressAcknowledgementCleanupV2 { supervisor, safety } = ingress_acknowledgement;
    readiness.remove_readiness_v2();
    drop(maintenance_ingress);
    if let Some(acknowledgement_safety) = safety {
        acknowledgement_safety.stop_v2().await;
    }
    shutdown_admission_acknowledging_process_v2(
        foundation,
        discord,
        lifecycle,
        supervisor,
        process_generation,
    )
    .await
}

async fn shutdown_orphaned_ingress_acknowledgement_v2(
    foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    retained: RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementCleanupV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    match retained {
        RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::Admission(lifecycle) => {
            shutdown_orphaned_admission_process_v2(
                foundation,
                discord,
                *lifecycle,
                ingress_acknowledgement,
                maintenance_ingress,
                readiness,
                process_generation,
            )
            .await
        }
        RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::EmptyOpenRefresh(lifecycle) => {
            shutdown_refreshing_empty_open_process_v2(
                foundation,
                discord,
                *lifecycle,
                ingress_acknowledgement,
                maintenance_ingress,
                readiness,
                process_generation,
            )
            .await
        }
    }
}

async fn shutdown_process_without_lifecycle_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementCleanupV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let RuntimeIngressAcknowledgementCleanupV2 { supervisor, safety } = ingress_acknowledgement;
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    if let Some(acknowledgement_safety) = safety {
        acknowledgement_safety.stop_v2().await;
    }
    shutdown_ingress_acknowledgement_supervisor_v2(&mut foundation, supervisor, cleanup_deadline)
        .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown = super::owner::finish_runtime_owner_held_process_shutdown_v1(
        Ok(RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped),
        foundation,
    );
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_frozen_empty_open_process_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoveryEmptyOpenProcessV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    ingress_acknowledgement: RuntimeProcessIngressAcknowledgementSupervisorV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    readiness.remove_readiness_v2();
    drop(maintenance_ingress);
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    shutdown_ingress_acknowledgement_supervisor_v2(
        &mut foundation,
        ingress_acknowledgement,
        cleanup_deadline,
    )
    .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.abort_and_shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_empty_open_process_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementCleanupV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let RuntimeIngressAcknowledgementCleanupV2 { supervisor, safety } = ingress_acknowledgement;
    readiness.remove_readiness_v2();
    drop(maintenance_ingress);
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    if let Some(acknowledgement_safety) = safety {
        acknowledgement_safety.stop_v2().await;
    }
    shutdown_ingress_acknowledgement_supervisor_v2(&mut foundation, supervisor, cleanup_deadline)
        .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
}

async fn shutdown_refreshing_empty_open_process_v2(
    mut foundation: RuntimeProcessFoundationV1,
    discord: RuntimeDiscordProcessSupervisorV2,
    lifecycle: crate::closed_recovery::RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2,
    ingress_acknowledgement: RuntimeIngressAcknowledgementCleanupV2,
    maintenance_ingress: RuntimeMaintenanceIngressGateOpenAuthorityV2,
    readiness: crate::health::RuntimeHealthReadinessPublisherV2,
    process_generation: NonZeroU64,
) -> Result<(), RuntimeClosedRecoveryProcessCleanupFailureV2> {
    let RuntimeIngressAcknowledgementCleanupV2 { supervisor, safety } = ingress_acknowledgement;
    readiness.remove_readiness_v2();
    drop(maintenance_ingress);
    let (cleanup_deadline, terminal) = foundation
        .begin_shutdown_v1(crate::RuntimeShutdownCauseV1::Explicit)
        .await;
    if let Some(acknowledgement_safety) = safety {
        acknowledgement_safety.stop_v2().await;
    }
    shutdown_ingress_acknowledgement_supervisor_v2(&mut foundation, supervisor, cleanup_deadline)
        .await;
    foundation.observe_shutdown_registry_v1();
    let discord_drain = foundation.gateway.begin_discord_drain_v1();
    let timing = foundation.lifecycle_timing_v2();
    let discord_shutdown = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord.shutdown_until(discord_drain, process_generation, cleanup_deadline),
        discord_shutdown_timing_outcome_v2,
    )
    .await
    .map_err(map_discord_shutdown_failure_v1);
    let owner = time_shutdown_result_v2(
        &timing,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        lifecycle.shutdown_until_v2(cleanup_deadline),
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let foundation = foundation.finish_shutdown_v1(cleanup_deadline).await;
    let owner_shutdown =
        super::owner::finish_runtime_owner_held_process_shutdown_v1(owner, foundation);
    let result =
        finish_paused_connected_shutdown_v1(discord_shutdown, owner_shutdown).map_err(Into::into);
    finish_observation_shutdown_timing_v2(terminal, result)
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

fn map_finalizer_activation_failure_v2(
    error: RuntimeProcessFinalizerActivationFailureV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match error {
        RuntimeProcessFinalizerActivationFailureV2::Unavailable => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerUnavailable
        }
        RuntimeProcessFinalizerActivationFailureV2::Reserve(_)
        | RuntimeProcessFinalizerActivationFailureV2::Activate(_)
        | RuntimeProcessFinalizerActivationFailureV2::NotReady(_) => {
            RuntimeProcessProductionHandoffFailureV2::FinalizerActivation
        }
    }
}

fn map_worker_production_handoff_failure_v2(
    error: RuntimeClosedRecoveryProductionHandoffErrorV2,
) -> RuntimeProcessProductionHandoffFailureV2 {
    match error {
        RuntimeClosedRecoveryProductionHandoffErrorV2::Owner => {
            RuntimeProcessProductionHandoffFailureV2::Owner
        }
        RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway => {
            RuntimeProcessProductionHandoffFailureV2::Gateway
        }
        RuntimeClosedRecoveryProductionHandoffErrorV2::Registry => {
            RuntimeProcessProductionHandoffFailureV2::Registry
        }
        RuntimeClosedRecoveryProductionHandoffErrorV2::Worker(_) => {
            RuntimeProcessProductionHandoffFailureV2::FixedPoint
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
        RuntimeClosedRecoveryFixedPointHandoffErrorV2::OwnerProcess(_) => {
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

impl Debug for RuntimeProcessBoundProductionHandoffProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessBoundProductionHandoffProcessV2(<redacted>)")
    }
}

impl Debug for RuntimeRecoveryResumeProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryResumeProcessV2(<redacted>)")
    }
}

impl Debug for RuntimeAdmissionAcknowledgingProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAdmissionAcknowledgingProcessV2(<redacted>)")
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
