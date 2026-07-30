use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU64, NonZeroU8};

use automation_runtime_controller::RuntimeBuildRevisionV1;
use automation_runtime_convergence::{ControllerId, ProcessInstanceId};
use automation_runtime_worker::{
    RuntimeMutationFinalizerGenerationV1, RuntimeRouteSetObservationV2,
};

use crate::build_revision::CompiledRuntimeBuildRevisionV1;
use crate::capability_readiness_supervisor::{
    RuntimeCapabilityReadinessActivationErrorV2, RuntimeCapabilityReadinessSupervisorExitV2,
};
use crate::controller_identity::generate_runtime_controller_id_v1;
use crate::database::{
    compose_runtime_database_dependencies_v1, RuntimeInteractionDispatchCompositionErrorV1,
    RuntimeInteractionDispatchDatabasePortV1,
};
use crate::health::{RuntimeHealthReadinessObserverV1, RuntimeHealthReadinessPublisherV2};
use crate::ingress_acknowledgement_supervisor::{
    RuntimeIngressAcknowledgementSupervisorConfigV2, RuntimeIngressAcknowledgementSupervisorExitV2,
    RuntimeIngressAcknowledgementSupervisorV2, RuntimeWorkerIngressAcknowledgementJobV2,
};
use crate::lifecycle_timing::{
    RuntimeLifecycleTimingMetricV2, RuntimeLifecycleTimingOutcomeV2,
    RuntimeLifecycleTimingRecorderV2, RuntimeLifecycleTimingTerminalReporterV2,
};
use crate::maintenance_ingress_gate::{
    RuntimeMaintenanceIngressGateControllerV2, RuntimeMaintenanceIngressGateObserverV2,
};
use crate::process_identity::generate_runtime_process_instance_id_v1;
use crate::process_supervisor::{
    RuntimeProcessRootSupervisorControlV2, RuntimeProcessRootSupervisorStartErrorV1,
    RuntimeProcessRootSupervisorV1,
};
use crate::startup::RuntimeStartupBudgetV1;
use crate::{
    compose_runtime_gateway_bootstrap_v1, compose_runtime_registry_bootstrap_v1,
    GatewayResourceConfigV1, ResolvedRuntimeSecretsV1, RuntimeConfigV1,
    RuntimeControllerIdGenerationErrorV1, RuntimeDatabaseCompositionErrorV1,
    RuntimeDatabaseDependenciesV1, RuntimeDatabasePoolShutdownErrorV1,
    RuntimeGatewayBootstrapErrorV1, RuntimeGatewayBootstrapV1,
    RuntimeMutationFinalizerStartErrorV1, RuntimeProcessInstanceIdGenerationErrorV1,
    RuntimeRegistryBootstrapErrorV1, RuntimeRegistryBootstrapV1,
    RuntimeRegistryRecoveryObservationErrorV1, RuntimeShutdownCauseV1, RuntimeSupervisorExitV1,
};

mod certification_finalizer;
mod closed;
pub(crate) mod connected;
mod execution;
#[cfg_attr(test, allow(dead_code))]
mod observation;
mod owner;
mod pending_drain_finalizer;
mod readiness;
mod recovery;
mod serving;
mod serving_certification;
mod startup_loop;

#[allow(unused_imports)]
pub(crate) use certification_finalizer::{
    complete_certification_finalizer_job_v2, RuntimeCertificationFinalizerCompletionFailureV2,
    RuntimeProcessCertificationFinalizerPortV2,
    RuntimeProductionCertificationFinalizationOutcomeV2,
    RuntimeProductionMutationFinalizerCompletionV3, RuntimeRegisteredCertificationFinalizerJobV2,
};
pub use closed::{
    RuntimeClosedRecoveryProcessCleanupFailureV2, RuntimeClosedRecoveryProcessShutdownErrorV2,
    RuntimeProcessClosedRecoveryCommitFailureV2, RuntimeProcessClosedRecoveryTransitionErrorV2,
    RuntimeProcessClosedRecoveryTransitionFailureV2, RuntimeProcessGatewayOwnerCommitFailureV2,
};
#[cfg(test)]
pub(crate) use execution::{
    execute_pending_drain_recovery_with_environment_v2, RuntimePendingDrainRecoveryEnvironmentV2,
    RuntimeStartupRecoveryExecutionAwaitFailureV2,
};
#[cfg(test)]
pub(crate) use observation::execute_recovery_resume_gateway_stage_v2;
pub(crate) use observation::RuntimeExactIngressAcknowledgementReobservationV3;
pub use observation::{
    RuntimeProcessProductionHandoffErrorV2, RuntimeProcessProductionHandoffFailureV2,
    RuntimeProcessStartupRecoveryObservationErrorV2,
    RuntimeProcessStartupRecoveryObservationFailureV2,
};
pub(crate) use owner::RuntimeOwnerHeldProcessV1;
pub use owner::{
    RuntimeGatewayOwnerShutdownFailureV1, RuntimeOwnerHeldProcessShutdownErrorV1,
    RuntimeProcessGatewayOwnerTransitionErrorV1,
};
#[cfg(test)]
pub(crate) use pending_drain_finalizer::{
    register_and_complete_pending_drain_job_v3, RuntimePendingDrainFinalizerDispatchFailureV3,
    RuntimePendingDrainFinalizerJobV3, RuntimePendingDrainFinalizerPortV3,
    RuntimePendingDrainMutationEnvironmentV3, RuntimePendingDrainMutationOutputV3,
    RuntimePendingDrainMutationStageV3,
};
pub use readiness::{
    RuntimeProcessRecoveryReadinessFailureV2, RuntimeProcessRecoveryReadinessTransitionErrorV2,
    RuntimeProcessRecoveryReadinessTransitionFailureV2,
};
pub use recovery::{
    RuntimeProcessClosedRecoveryBeginFailureV2, RuntimeProcessGatewayOwnerPrepareFailureV2,
    RuntimeProcessRecoveryPendingTransitionErrorV2,
    RuntimeProcessRecoveryPendingTransitionFailureV2,
    RuntimeRecoveryPendingProcessCleanupFailureV2, RuntimeRecoveryPendingProcessShutdownErrorV2,
};
pub use startup_loop::{
    RuntimeProcessStartupRecoveryLoopErrorV2, RuntimeProcessStartupRecoveryLoopFailureV2,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessFoundationCompositionErrorV1 {
    #[error("runtime process foundation instance identity generation failed")]
    ProcessInstanceId(RuntimeProcessInstanceIdGenerationErrorV1),
    #[error("runtime process foundation controller identity generation failed")]
    ControllerId(RuntimeControllerIdGenerationErrorV1),
    #[error("runtime process foundation database composition failed")]
    Database(RuntimeDatabaseCompositionErrorV1),
    #[error("runtime process foundation registry composition failed")]
    Registry(RuntimeRegistryBootstrapErrorV1),
    #[error("runtime process foundation gateway composition failed")]
    Gateway(RuntimeGatewayBootstrapErrorV1),
    #[error("runtime process foundation interaction dispatch composition failed")]
    InteractionDispatch(RuntimeInteractionDispatchCompositionErrorV1),
    #[error("runtime process foundation mutation finalizer composition failed")]
    MutationFinalizer(RuntimeMutationFinalizerStartErrorV1),
    #[error("runtime process foundation shutdown signal composition failed")]
    ShutdownSignal,
    #[error("runtime process foundation health listener composition failed")]
    HealthListener,
    #[error("runtime process foundation registry failure cleanup failed")]
    CleanupAfterRegistry {
        composition: RuntimeRegistryBootstrapErrorV1,
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
    #[error("runtime process foundation gateway failure cleanup failed")]
    CleanupAfterGateway {
        composition: RuntimeGatewayBootstrapErrorV1,
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
    #[error("runtime process foundation interaction dispatch failure cleanup failed")]
    CleanupAfterInteractionDispatch {
        composition: RuntimeInteractionDispatchCompositionErrorV1,
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
    #[error("runtime process foundation mutation finalizer failure cleanup failed")]
    CleanupAfterMutationFinalizer {
        composition: RuntimeMutationFinalizerStartErrorV1,
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
    #[error("runtime process foundation root supervisor failure cleanup failed")]
    CleanupAfterRootSupervisor {
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
    #[error("runtime process foundation startup operation deadline elapsed")]
    OperationDeadlineElapsed,
    #[error("runtime process foundation startup deadline cleanup failed")]
    CleanupAfterOperationDeadline {
        cleanup: RuntimeDatabasePoolShutdownErrorV1,
    },
}

impl RuntimeProcessFoundationCompositionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProcessInstanceId(error) => error.code(),
            Self::ControllerId(error) => error.code(),
            Self::Database(error) => error.code(),
            Self::Registry(error) => error.code(),
            Self::Gateway(error) => error.code(),
            Self::InteractionDispatch(error) => error.code(),
            Self::MutationFinalizer(_) => "runtime_process_foundation_mutation_finalizer",
            Self::ShutdownSignal => "runtime_process_foundation_shutdown_signal",
            Self::HealthListener => "runtime_process_foundation_health_listener",
            Self::CleanupAfterRegistry { .. } => {
                "runtime_process_foundation_cleanup_after_registry"
            }
            Self::CleanupAfterGateway { .. } => "runtime_process_foundation_cleanup_after_gateway",
            Self::CleanupAfterInteractionDispatch { .. } => {
                "runtime_process_foundation_cleanup_after_interaction_dispatch"
            }
            Self::CleanupAfterMutationFinalizer { .. } => {
                "runtime_process_foundation_cleanup_after_mutation_finalizer"
            }
            Self::CleanupAfterRootSupervisor { .. } => {
                "runtime_process_foundation_cleanup_after_root_supervisor"
            }
            Self::OperationDeadlineElapsed => {
                "runtime_process_foundation_operation_deadline_elapsed"
            }
            Self::CleanupAfterOperationDeadline { .. } => {
                "runtime_process_foundation_cleanup_after_operation_deadline"
            }
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::ProcessInstanceId(error) => error.context(),
            Self::ControllerId(error) => error.context(),
            Self::Database(error) => error.context(),
            Self::Registry(_)
            | Self::Gateway(_)
            | Self::InteractionDispatch(_)
            | Self::MutationFinalizer(_)
            | Self::ShutdownSignal
            | Self::HealthListener
            | Self::CleanupAfterRegistry { .. }
            | Self::CleanupAfterGateway { .. }
            | Self::CleanupAfterInteractionDispatch { .. }
            | Self::CleanupAfterMutationFinalizer { .. }
            | Self::CleanupAfterRootSupervisor { .. }
            | Self::OperationDeadlineElapsed
            | Self::CleanupAfterOperationDeadline { .. } => None,
        }
    }
}

impl Debug for RuntimeProcessFoundationCompositionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationCompositionErrorV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProcessFoundationShutdownFailureV1 {
    Finalizer(RuntimeSupervisorExitV1),
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    SignalSupervisor,
    Database(RuntimeDatabasePoolShutdownErrorV1),
    HealthListener,
    IngressAcknowledgement,
    CapabilityReadiness,
    RuntimeController,
}

impl RuntimeProcessFoundationShutdownFailureV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Finalizer(exit) => exit.code(),
            Self::Registry(error) => error.code(),
            Self::SignalSupervisor => "runtime_process_signal_supervisor_shutdown",
            Self::Database(RuntimeDatabasePoolShutdownErrorV1::TimedOut) => {
                "runtime_process_foundation_database_shutdown_timed_out"
            }
            Self::HealthListener => "runtime_process_health_listener_shutdown",
            Self::IngressAcknowledgement => {
                "runtime_process_ingress_acknowledgement_supervisor_shutdown"
            }
            Self::CapabilityReadiness => "runtime_process_capability_readiness_supervisor_shutdown",
            Self::RuntimeController => "runtime_process_serving_controller_shutdown",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("runtime process foundation shutdown failed")]
pub struct RuntimeProcessFoundationShutdownErrorV1 {
    primary: RuntimeProcessFoundationShutdownFailureV1,
    failure_count: NonZeroU8,
}

impl RuntimeProcessFoundationShutdownErrorV1 {
    #[cfg(test)]
    pub(crate) const fn single(primary: RuntimeProcessFoundationShutdownFailureV1) -> Self {
        Self {
            primary,
            failure_count: NonZeroU8::MIN,
        }
    }

    pub const fn code(self) -> &'static str {
        if self.failure_count.get() == 1 {
            self.primary.code()
        } else {
            "runtime_process_foundation_multiple_shutdown_failures"
        }
    }

    pub const fn primary(self) -> RuntimeProcessFoundationShutdownFailureV1 {
        self.primary
    }

    pub const fn failure_count(self) -> NonZeroU8 {
        self.failure_count
    }

    pub const fn database_only(self) -> Option<RuntimeDatabasePoolShutdownErrorV1> {
        match (self.failure_count.get(), self.primary) {
            (1, RuntimeProcessFoundationShutdownFailureV1::Database(error)) => Some(error),
            _ => None,
        }
    }
}

impl Debug for RuntimeProcessFoundationShutdownErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationShutdownErrorV1(<redacted>)")
    }
}

struct RuntimeProcessFoundationShutdownFailuresV1 {
    primary: Option<RuntimeProcessFoundationShutdownFailureV1>,
    failure_count: u8,
}

impl RuntimeProcessFoundationShutdownFailuresV1 {
    const fn new() -> Self {
        Self {
            primary: None,
            failure_count: 0,
        }
    }

    fn record(&mut self, failure: RuntimeProcessFoundationShutdownFailureV1) {
        self.primary.get_or_insert(failure);
        self.failure_count = self.failure_count.saturating_add(1);
    }

    fn finish(self) -> Result<(), RuntimeProcessFoundationShutdownErrorV1> {
        match self.primary {
            Some(primary) => Err(RuntimeProcessFoundationShutdownErrorV1 {
                primary,
                failure_count: NonZeroU8::new(self.failure_count).unwrap_or(NonZeroU8::MIN),
            }),
            None => Ok(()),
        }
    }
}

type RuntimeProcessStartupMutationFinalizerV3 =
    certification_finalizer::RuntimeProcessMutationFinalizerSupervisorV3<
        execution::RuntimeProductionPendingDrainFinalizerEnvironmentV3,
    >;

type RuntimeProcessMutationFinalizerV3 =
    certification_finalizer::RuntimeProcessMutationFinalizerProcessSupervisorV3<
        execution::RuntimeProductionPendingDrainFinalizerEnvironmentV3,
    >;

pub(super) type RuntimeProcessIngressAcknowledgementJobV2 =
    RuntimeWorkerIngressAcknowledgementJobV2<
        crate::closed_recovery::RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2,
    >;

pub(super) type RuntimeProcessIngressAcknowledgementSupervisorV2 =
    RuntimeIngressAcknowledgementSupervisorV2<
        automation_runtime_execution_postgres::PostgresRuntimeExecutionV1,
        RuntimeProcessIngressAcknowledgementJobV2,
    >;

pub(crate) struct RuntimeProcessFoundationV1 {
    gateway: RuntimeGatewayBootstrapV1,
    registry: RuntimeRegistryBootstrapV1,
    interaction_dispatch_port: RuntimeInteractionDispatchDatabasePortV1,
    databases: RuntimeDatabaseDependenciesV1,
    secrets: ResolvedRuntimeSecretsV1,
    config: RuntimeConfigV1,
    startup_budget: RuntimeStartupBudgetV1,
    build_revision: RuntimeBuildRevisionV1,
    process_instance_id: ProcessInstanceId,
    controller_id: ControllerId,
    mutation_finalizer: Option<RuntimeProcessStartupMutationFinalizerV3>,
    process_mutation_finalizer: Option<RuntimeProcessMutationFinalizerV3>,
    maintenance_ingress: Option<RuntimeMaintenanceIngressGateControllerV2>,
    maintenance_ingress_observer: RuntimeMaintenanceIngressGateObserverV2,
    readiness_publisher: Option<RuntimeHealthReadinessPublisherV2>,
    ingress_acknowledgement: Option<RuntimeProcessIngressAcknowledgementSupervisorV2>,
    lifecycle_timing: RuntimeLifecycleTimingRecorderV2,
    lifecycle_timing_terminal: Option<RuntimeLifecycleTimingTerminalReporterV2>,
    cleanup_mode: RuntimeProcessFoundationCleanupModeV1,
    root_supervisor: RuntimeProcessRootSupervisorV1,
    shutdown_failures: RuntimeProcessFoundationShutdownFailuresV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeProcessFoundationCleanupModeV1 {
    StartupBound,
    ProcessBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeProcessFinalizerHandoffFailureV2 {
    DeadlineElapsed,
    Unavailable,
    Terminal(RuntimeSupervisorExitV1),
    NotSettled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeProcessFinalizerActivationFailureV2 {
    Unavailable,
    Reserve(crate::mutation_finalizer::RuntimeMutationFinalizerProcessActivationReserveErrorV1),
    Activate(crate::mutation_finalizer::RuntimeMutationFinalizerProcessActivationErrorV1),
    NotReady(crate::mutation_finalizer::RuntimeMutationFinalizerProcessIntakeHealthV1),
}

impl RuntimeProcessFoundationV1 {
    pub(super) fn shutdown_observer_v1(&self) -> crate::shutdown::RuntimeShutdownObserverV1 {
        self.root_supervisor.observer()
    }

    pub(super) fn lifecycle_timing_v2(&self) -> RuntimeLifecycleTimingRecorderV2 {
        self.lifecycle_timing.clone()
    }

    pub(super) fn shutdown_trigger_v1(
        &self,
    ) -> crate::process_supervisor::RuntimeProcessShutdownTriggerV1 {
        self.root_supervisor.shutdown_trigger()
    }

    pub(super) fn invalidation_trigger_v1(
        &self,
    ) -> crate::process_supervisor::RuntimeProcessInvalidationTriggerV1 {
        self.root_supervisor.invalidation_trigger()
    }

    pub(super) fn product_readiness_observer_v1(&self) -> RuntimeHealthReadinessObserverV1 {
        self.root_supervisor.readiness_observer_v1()
    }

    pub(super) fn interaction_dispatch_port_v1(&self) -> RuntimeInteractionDispatchDatabasePortV1 {
        self.interaction_dispatch_port.clone()
    }

    pub(super) fn trip_shutdown_v1(
        &self,
        cause: RuntimeShutdownCauseV1,
    ) -> crate::RuntimeShutdownObservationV1 {
        self.root_supervisor.trip(cause).observation()
    }

    pub(super) fn effective_shutdown_deadline_v1(
        &self,
        observation: crate::RuntimeShutdownObservationV1,
    ) -> std::time::Instant {
        match self.cleanup_mode {
            RuntimeProcessFoundationCleanupModeV1::StartupBound => observation
                .deadline()
                .min(self.startup_budget.cleanup_deadline()),
            RuntimeProcessFoundationCleanupModeV1::ProcessBound => observation.deadline(),
        }
    }

    pub(super) fn enter_process_cleanup_mode_v2(&mut self) -> bool {
        if self.cleanup_mode != RuntimeProcessFoundationCleanupModeV1::StartupBound {
            return false;
        }
        self.cleanup_mode = RuntimeProcessFoundationCleanupModeV1::ProcessBound;
        true
    }

    pub(super) fn maintenance_ingress_controller_v2(
        &mut self,
    ) -> Option<RuntimeMaintenanceIngressGateControllerV2> {
        self.maintenance_ingress.take()
    }

    pub(super) fn maintenance_ingress_observer_v2(
        &self,
    ) -> RuntimeMaintenanceIngressGateObserverV2 {
        self.maintenance_ingress_observer.clone()
    }

    pub(super) fn take_readiness_publisher_v2(
        &mut self,
    ) -> Option<RuntimeHealthReadinessPublisherV2> {
        self.readiness_publisher.take()
    }

    pub(super) fn take_ingress_acknowledgement_supervisor_v2(
        &mut self,
    ) -> Option<RuntimeProcessIngressAcknowledgementSupervisorV2> {
        self.ingress_acknowledgement.take()
    }

    pub(super) async fn activate_capability_readiness_supervisor_v2(
        &mut self,
        deadline: std::time::Instant,
    ) -> Result<(), RuntimeCapabilityReadinessActivationErrorV2> {
        let probe = self.databases.readiness_probe_v2();
        self.root_supervisor
            .activate_capability_readiness_until_v2(probe, deadline)
            .await
    }

    pub(super) async fn seal_startup_finalizer_for_handoff_v2(
        &mut self,
        cutoff: std::time::Instant,
    ) -> Result<
        crate::RuntimeMutationFinalizerHandoffStateV1,
        RuntimeProcessFinalizerHandoffFailureV2,
    > {
        if std::time::Instant::now() >= cutoff {
            return Err(RuntimeProcessFinalizerHandoffFailureV2::DeadlineElapsed);
        }
        let finalizer = self
            .mutation_finalizer
            .as_mut()
            .ok_or(RuntimeProcessFinalizerHandoffFailureV2::Unavailable)?;
        finalizer.seal_intake();
        let settled = tokio::time::timeout_at(
            tokio::time::Instant::from_std(cutoff),
            finalizer.wait_startup_jobs_settled(),
        )
        .await
        .map_err(|_| RuntimeProcessFinalizerHandoffFailureV2::DeadlineElapsed)?;
        let snapshot = finalizer.snapshot();
        if let Some(exit) = snapshot.terminal() {
            return Err(RuntimeProcessFinalizerHandoffFailureV2::Terminal(exit));
        }
        let handoff = snapshot.handoff_state();
        if !settled || !handoff.startup_intake_sealed() || !handoff.startup_jobs_settled() {
            return Err(RuntimeProcessFinalizerHandoffFailureV2::NotSettled);
        }
        Ok(handoff)
    }

    pub(super) fn revalidate_finalizer_handoff_v2(
        &self,
        expected: crate::RuntimeMutationFinalizerHandoffStateV1,
    ) -> Result<(), RuntimeProcessFinalizerHandoffFailureV2> {
        let finalizer = self
            .mutation_finalizer
            .as_ref()
            .ok_or(RuntimeProcessFinalizerHandoffFailureV2::Unavailable)?;
        let snapshot = finalizer.snapshot();
        if let Some(exit) = snapshot.terminal() {
            return Err(RuntimeProcessFinalizerHandoffFailureV2::Terminal(exit));
        }
        let observed = snapshot.handoff_state();
        if observed != expected
            || !observed.startup_intake_sealed()
            || !observed.startup_jobs_settled()
        {
            return Err(RuntimeProcessFinalizerHandoffFailureV2::NotSettled);
        }
        Ok(())
    }

    pub(super) async fn activate_process_finalizer_until_v2(
        &mut self,
        deadline: std::time::Instant,
    ) -> Result<RuntimeMutationFinalizerGenerationV1, RuntimeProcessFinalizerActivationFailureV2>
    {
        let supervisor = self
            .mutation_finalizer
            .take()
            .ok_or(RuntimeProcessFinalizerActivationFailureV2::Unavailable)?;
        let activation = match supervisor.reserve_process_activation() {
            Ok(activation) => activation,
            Err(error) => {
                self.mutation_finalizer = Some(supervisor);
                return Err(RuntimeProcessFinalizerActivationFailureV2::Reserve(error));
            }
        };
        let generation = activation.generation();
        match supervisor
            .activate_process_until(activation, deadline)
            .await
        {
            Ok(process) => {
                let health = process.process_intake_health();
                if !health.is_ready() {
                    self.process_mutation_finalizer = Some(process);
                    return Err(RuntimeProcessFinalizerActivationFailureV2::NotReady(health));
                }
                self.process_mutation_finalizer = Some(process);
                Ok(generation)
            }
            Err(failure) => {
                let error = failure.error();
                self.mutation_finalizer = Some(failure.into_shutdown_supervisor());
                Err(RuntimeProcessFinalizerActivationFailureV2::Activate(error))
            }
        }
    }

    pub(super) fn process_finalizer_health_v2(
        &self,
    ) -> Option<crate::mutation_finalizer::RuntimeMutationFinalizerProcessIntakeHealthV1> {
        self.process_mutation_finalizer
            .as_ref()
            .map(|process| process.process_intake_health())
    }

    #[allow(dead_code)]
    pub(super) fn certification_finalizer_port_v2(
        &self,
    ) -> Option<RuntimeProcessCertificationFinalizerPortV2<'_>> {
        self.process_mutation_finalizer
            .as_ref()
            .map(RuntimeProcessCertificationFinalizerPortV2::new)
    }

    #[allow(dead_code)]
    pub(super) async fn next_process_mutation_finalizer_completion_v3(
        &mut self,
    ) -> Option<RuntimeProductionMutationFinalizerCompletionV3> {
        match self.process_mutation_finalizer.as_mut() {
            Some(process) => process
                .next_completion()
                .await
                .map(RuntimeProductionMutationFinalizerCompletionV3::new),
            None => None,
        }
    }

    pub(super) async fn begin_shutdown_v1(
        &mut self,
        cause: RuntimeShutdownCauseV1,
    ) -> (std::time::Instant, RuntimeLifecycleTimingTerminalReporterV2) {
        let terminal = self
            .lifecycle_timing_terminal
            .take()
            .expect("runtime lifecycle terminal reporter");
        let observation = self.trip_shutdown_v1(cause);
        let deadline = self.effective_shutdown_deadline_v1(observation);
        let finalizer_present =
            self.mutation_finalizer.is_some() || self.process_mutation_finalizer.is_some();
        let finalizer_timing = finalizer_present.then(|| {
            self.lifecycle_timing
                .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownFinalizerJoin)
        });
        let mut finalizer_outcome = RuntimeLifecycleTimingOutcomeV2::Completed;
        if let Some(mutation_finalizer) = self.mutation_finalizer.take() {
            let finalizer_report = mutation_finalizer.shutdown_until(deadline).await;
            finalizer_outcome = merge_lifecycle_timing_outcome_v2(
                finalizer_outcome,
                finalizer_shutdown_timing_outcome_v2(finalizer_report.exit()),
            );
            if finalizer_report.exit() != RuntimeSupervisorExitV1::Commanded {
                self.shutdown_failures.record(
                    RuntimeProcessFoundationShutdownFailureV1::Finalizer(finalizer_report.exit()),
                );
            }
            drop(finalizer_report);
        }
        if let Some(mutation_finalizer) = self.process_mutation_finalizer.take() {
            let finalizer_report = mutation_finalizer.shutdown_until(deadline).await;
            finalizer_outcome = merge_lifecycle_timing_outcome_v2(
                finalizer_outcome,
                finalizer_shutdown_timing_outcome_v2(finalizer_report.exit()),
            );
            if finalizer_report.exit() != RuntimeSupervisorExitV1::Commanded {
                self.shutdown_failures.record(
                    RuntimeProcessFoundationShutdownFailureV1::Finalizer(finalizer_report.exit()),
                );
            }
            drop(finalizer_report);
        }
        if let Some(timing) = finalizer_timing {
            timing.finish_v2(finalizer_outcome);
        }
        if let Some(ingress_acknowledgement) = self.ingress_acknowledgement.take() {
            let timing = self
                .lifecycle_timing
                .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownIngressAcknowledgementJoin);
            let report = ingress_acknowledgement.shutdown_until(deadline).await;
            let outcome = ingress_acknowledgement_shutdown_timing_outcome_v2(
                report.exit(),
                report.completion().is_some(),
            );
            if report.exit() != RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
                || report.completion().is_some()
            {
                self.shutdown_failures
                    .record(RuntimeProcessFoundationShutdownFailureV1::IngressAcknowledgement);
            }
            drop(report);
            timing.finish_v2(outcome);
        }
        let capability_timing = self
            .lifecycle_timing
            .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownCapabilityReadinessJoin);
        let capability_readiness = self
            .root_supervisor
            .shutdown_capability_readiness_until_v2(deadline)
            .await;
        if !matches!(
            capability_readiness,
            RuntimeCapabilityReadinessSupervisorExitV2::Commanded
                | RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        ) {
            self.shutdown_failures
                .record(RuntimeProcessFoundationShutdownFailureV1::CapabilityReadiness);
        }
        capability_timing.finish_v2(capability_readiness_shutdown_timing_outcome_v2(
            capability_readiness,
        ));
        (deadline, terminal)
    }

    pub(super) fn observe_shutdown_registry_v1(&mut self) {
        let timing = self
            .lifecycle_timing
            .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownRegistryObservation);
        match self.registry.observe_recovery_empty_projection_v2() {
            Ok(_) => timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::Completed),
            Err(error) => {
                self.shutdown_failures
                    .record(RuntimeProcessFoundationShutdownFailureV1::Registry(error));
                timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::FailedClosed);
            }
        }
    }

    pub(super) fn observe_shutdown_serving_registry_v2(
        &mut self,
        observation: Result<
            RuntimeRouteSetObservationV2,
            RuntimeRegistryRecoveryObservationErrorV1,
        >,
    ) {
        let timing = self
            .lifecycle_timing
            .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownRegistryObservation);
        match accept_shutdown_serving_registry_observation_v2(observation) {
            Ok(()) => timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::Completed),
            Err(error) => {
                self.shutdown_failures
                    .record(RuntimeProcessFoundationShutdownFailureV1::Registry(error));
                timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::FailedClosed);
            }
        }
    }

    pub(super) fn observe_shutdown_serving_registry_without_lifecycle_v2(&mut self) {
        let observation = self.registry.observe_shutdown_route_set_v2();
        self.observe_shutdown_serving_registry_v2(observation);
    }

    pub(super) fn record_finalizer_shutdown_exit_v1(&mut self, exit: RuntimeSupervisorExitV1) {
        if exit != RuntimeSupervisorExitV1::Commanded {
            self.shutdown_failures
                .record(RuntimeProcessFoundationShutdownFailureV1::Finalizer(exit));
        }
    }

    pub(super) fn record_ingress_acknowledgement_shutdown_v2(
        &mut self,
        exit: RuntimeIngressAcknowledgementSupervisorExitV2,
        completion_present: bool,
    ) {
        if exit != RuntimeIngressAcknowledgementSupervisorExitV2::Commanded || completion_present {
            self.shutdown_failures
                .record(RuntimeProcessFoundationShutdownFailureV1::IngressAcknowledgement);
        }
    }

    pub(super) fn record_runtime_controller_shutdown_failure_v2(&mut self) {
        self.shutdown_failures
            .record(RuntimeProcessFoundationShutdownFailureV1::RuntimeController);
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), RuntimeProcessFoundationShutdownErrorV1> {
        let (cleanup_deadline, terminal) = self
            .begin_shutdown_v1(RuntimeShutdownCauseV1::Explicit)
            .await;
        self.observe_shutdown_registry_v1();
        let result = self.finish_shutdown_v1(cleanup_deadline).await;
        let outcome = if result.is_ok() {
            RuntimeLifecycleTimingOutcomeV2::Completed
        } else {
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        };
        terminal.finish_result_v2(result, outcome)
    }

    pub(super) async fn finish_shutdown_v1(
        self,
        cleanup_deadline: std::time::Instant,
    ) -> Result<(), RuntimeProcessFoundationShutdownErrorV1> {
        let Self {
            gateway,
            registry,
            interaction_dispatch_port,
            databases,
            secrets,
            config,
            startup_budget,
            build_revision,
            process_instance_id,
            controller_id,
            mutation_finalizer,
            process_mutation_finalizer,
            maintenance_ingress,
            maintenance_ingress_observer,
            readiness_publisher,
            ingress_acknowledgement,
            lifecycle_timing,
            lifecycle_timing_terminal,
            cleanup_mode,
            mut root_supervisor,
            mut shutdown_failures,
        } = self;
        let finalizer_present =
            mutation_finalizer.is_some() || process_mutation_finalizer.is_some();
        let finalizer_timing = finalizer_present.then(|| {
            lifecycle_timing.start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownFinalizerJoin)
        });
        let mut finalizer_outcome = RuntimeLifecycleTimingOutcomeV2::Completed;
        if let Some(mutation_finalizer) = mutation_finalizer {
            let finalizer_report = mutation_finalizer.shutdown_until(cleanup_deadline).await;
            finalizer_outcome = merge_lifecycle_timing_outcome_v2(
                finalizer_outcome,
                finalizer_shutdown_timing_outcome_v2(finalizer_report.exit()),
            );
            if finalizer_report.exit() != RuntimeSupervisorExitV1::Commanded {
                shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::Finalizer(
                    finalizer_report.exit(),
                ));
            }
            drop(finalizer_report);
        }
        if let Some(mutation_finalizer) = process_mutation_finalizer {
            let finalizer_report = mutation_finalizer.shutdown_until(cleanup_deadline).await;
            finalizer_outcome = merge_lifecycle_timing_outcome_v2(
                finalizer_outcome,
                finalizer_shutdown_timing_outcome_v2(finalizer_report.exit()),
            );
            if finalizer_report.exit() != RuntimeSupervisorExitV1::Commanded {
                shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::Finalizer(
                    finalizer_report.exit(),
                ));
            }
            drop(finalizer_report);
        }
        if let Some(timing) = finalizer_timing {
            timing.finish_v2(finalizer_outcome);
        }
        if let Some(ingress_acknowledgement) = ingress_acknowledgement {
            let timing = lifecycle_timing
                .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownIngressAcknowledgementJoin);
            let report = ingress_acknowledgement
                .shutdown_until(cleanup_deadline)
                .await;
            let outcome = ingress_acknowledgement_shutdown_timing_outcome_v2(
                report.exit(),
                report.completion().is_some(),
            );
            if report.exit() != RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
                || report.completion().is_some()
            {
                shutdown_failures
                    .record(RuntimeProcessFoundationShutdownFailureV1::IngressAcknowledgement);
            }
            drop(report);
            timing.finish_v2(outcome);
        }
        let capability_timing = lifecycle_timing
            .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownCapabilityReadinessJoin);
        let capability_readiness = root_supervisor
            .shutdown_capability_readiness_until_v2(cleanup_deadline)
            .await;
        if !matches!(
            capability_readiness,
            RuntimeCapabilityReadinessSupervisorExitV2::Commanded
                | RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        ) {
            shutdown_failures
                .record(RuntimeProcessFoundationShutdownFailureV1::CapabilityReadiness);
        }
        capability_timing.finish_v2(capability_readiness_shutdown_timing_outcome_v2(
            capability_readiness,
        ));
        let signal_timing =
            lifecycle_timing.start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownRootSignalJoin);
        let signal_exit = root_supervisor.join_signal_until(cleanup_deadline).await;
        if matches!(
            signal_exit,
            crate::process_supervisor::RuntimeProcessSignalTaskExitV1::StreamClosed
                | crate::process_supervisor::RuntimeProcessSignalTaskExitV1::Panicked
        ) {
            shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::SignalSupervisor);
        }
        signal_timing.finish_v2(
            if matches!(
                signal_exit,
                crate::process_supervisor::RuntimeProcessSignalTaskExitV1::StreamClosed
                    | crate::process_supervisor::RuntimeProcessSignalTaskExitV1::Panicked
            ) {
                RuntimeLifecycleTimingOutcomeV2::FailedClosed
            } else {
                RuntimeLifecycleTimingOutcomeV2::Completed
            },
        );
        let database_timing = lifecycle_timing
            .start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownDatabasePoolsClose);
        let shutdown = databases.shutdown();
        drop((gateway, registry, interaction_dispatch_port, databases));
        match shutdown.close_until(cleanup_deadline).await {
            Ok(()) => database_timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::Completed),
            Err(error) => {
                shutdown_failures
                    .record(RuntimeProcessFoundationShutdownFailureV1::Database(error));
                database_timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed);
            }
        }
        drop(shutdown);
        finish_runtime_process_foundation_shutdown_v1(
            secrets,
            (
                config,
                startup_budget,
                build_revision,
                process_instance_id,
                controller_id,
                maintenance_ingress,
                maintenance_ingress_observer,
                readiness_publisher,
                cleanup_mode,
            ),
            (),
        );
        let health_timing =
            lifecycle_timing.start_span_v2(RuntimeLifecycleTimingMetricV2::ShutdownHealthStop);
        match root_supervisor
            .shutdown_health_until(cleanup_deadline)
            .await
        {
            Ok(()) => health_timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::Completed),
            Err(crate::health::RuntimeHealthShutdownErrorV1::DeadlineElapsed) => {
                shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::HealthListener);
                health_timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed);
            }
            Err(crate::health::RuntimeHealthShutdownErrorV1::TaskStopped) => {
                shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::HealthListener);
                health_timing.finish_v2(RuntimeLifecycleTimingOutcomeV2::FailedClosed);
            }
        }
        for metric in [
            RuntimeLifecycleTimingMetricV2::ShutdownFinalizerJoin,
            RuntimeLifecycleTimingMetricV2::ShutdownIngressAcknowledgementJoin,
            RuntimeLifecycleTimingMetricV2::ShutdownCapabilityReadinessJoin,
            RuntimeLifecycleTimingMetricV2::ShutdownRegistryObservation,
            RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
            RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        ] {
            lifecycle_timing.record_skipped_v2(metric);
        }
        let result = shutdown_failures.finish();
        drop(lifecycle_timing_terminal);
        result
    }
}

fn merge_lifecycle_timing_outcome_v2(
    current: RuntimeLifecycleTimingOutcomeV2,
    next: RuntimeLifecycleTimingOutcomeV2,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match (current, next) {
        (RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed, _)
        | (_, RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed) => {
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        }
        (RuntimeLifecycleTimingOutcomeV2::FailedClosed, _)
        | (_, RuntimeLifecycleTimingOutcomeV2::FailedClosed) => {
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        }
        (_, next) => next,
    }
}

fn finalizer_shutdown_timing_outcome_v2(
    exit: RuntimeSupervisorExitV1,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match exit {
        RuntimeSupervisorExitV1::Commanded => RuntimeLifecycleTimingOutcomeV2::Completed,
        RuntimeSupervisorExitV1::DeadlineElapsed => {
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        }
        RuntimeSupervisorExitV1::DependencyTerminal
        | RuntimeSupervisorExitV1::ProtocolViolation
        | RuntimeSupervisorExitV1::Panicked
        | RuntimeSupervisorExitV1::Aborted => RuntimeLifecycleTimingOutcomeV2::FailedClosed,
    }
}

fn ingress_acknowledgement_shutdown_timing_outcome_v2(
    exit: RuntimeIngressAcknowledgementSupervisorExitV2,
    completion_present: bool,
) -> RuntimeLifecycleTimingOutcomeV2 {
    if completion_present {
        return RuntimeLifecycleTimingOutcomeV2::FailedClosed;
    }
    match exit {
        RuntimeIngressAcknowledgementSupervisorExitV2::Commanded => {
            RuntimeLifecycleTimingOutcomeV2::Completed
        }
        RuntimeIngressAcknowledgementSupervisorExitV2::DeadlineElapsed => {
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        }
        RuntimeIngressAcknowledgementSupervisorExitV2::IntakeClosed
        | RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation
        | RuntimeIngressAcknowledgementSupervisorExitV2::Panicked
        | RuntimeIngressAcknowledgementSupervisorExitV2::Aborted => {
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        }
    }
}

fn capability_readiness_shutdown_timing_outcome_v2(
    exit: RuntimeCapabilityReadinessSupervisorExitV2,
) -> RuntimeLifecycleTimingOutcomeV2 {
    match exit {
        RuntimeCapabilityReadinessSupervisorExitV2::Commanded
        | RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost => {
            RuntimeLifecycleTimingOutcomeV2::Completed
        }
        RuntimeCapabilityReadinessSupervisorExitV2::DeadlineElapsed => {
            RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
        }
        RuntimeCapabilityReadinessSupervisorExitV2::ControlClosed
        | RuntimeCapabilityReadinessSupervisorExitV2::Panicked => {
            RuntimeLifecycleTimingOutcomeV2::FailedClosed
        }
    }
}

fn finish_runtime_process_foundation_shutdown_v1<S, R, T>(secrets: S, retained: R, result: T) -> T {
    drop(secrets);
    drop(retained);
    result
}

impl Debug for RuntimeProcessFoundationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationV1(<redacted>)")
    }
}

pub(crate) async fn compose_runtime_process_foundation_v1(
    startup_budget: RuntimeStartupBudgetV1,
    config: RuntimeConfigV1,
    secrets: ResolvedRuntimeSecretsV1,
    build_revision: CompiledRuntimeBuildRevisionV1,
) -> Result<RuntimeProcessFoundationV1, RuntimeProcessFoundationCompositionErrorV1> {
    if !startup_budget.operation_is_open() {
        return Err(RuntimeProcessFoundationCompositionErrorV1::OperationDeadlineElapsed);
    }
    let process_instance_id = generate_runtime_process_instance_id_v1();
    if !startup_budget.operation_is_open() {
        return Err(RuntimeProcessFoundationCompositionErrorV1::OperationDeadlineElapsed);
    }
    let process_instance_id = process_instance_id
        .map_err(RuntimeProcessFoundationCompositionErrorV1::ProcessInstanceId)?;
    let controller_id = generate_runtime_controller_id_v1();
    if !startup_budget.operation_is_open() {
        return Err(RuntimeProcessFoundationCompositionErrorV1::OperationDeadlineElapsed);
    }
    let controller_id =
        controller_id.map_err(RuntimeProcessFoundationCompositionErrorV1::ControllerId)?;
    let build_revision = build_revision.into_revision();
    let (lifecycle_timing, lifecycle_timing_observer) =
        RuntimeLifecycleTimingRecorderV2::create_v2();
    let lifecycle_timing_terminal = RuntimeLifecycleTimingTerminalReporterV2::new_v2(
        lifecycle_timing.clone(),
        lifecycle_timing_observer,
    );
    let databases = compose_runtime_database_dependencies_v1(&config, &secrets, &startup_budget)
        .await
        .map_err(RuntimeProcessFoundationCompositionErrorV1::Database)?;
    if !startup_budget.operation_is_open() {
        return Err(cleanup_after_operation_deadline_v1(databases, &startup_budget).await);
    }
    let closed_components =
        compose_closed_process_components_v1(&process_instance_id, config.gateway());
    if !startup_budget.operation_is_open() {
        drop(closed_components);
        return Err(cleanup_after_operation_deadline_v1(databases, &startup_budget).await);
    }
    let closed_components = match closed_components {
        Ok(components) => components,
        Err(error) => {
            let shutdown = databases.shutdown();
            drop(databases);
            let cleanup = shutdown
                .close_until(startup_budget.cleanup_deadline())
                .await
                .err();
            return Err(error.into_public(cleanup));
        }
    };
    closed_components
        .gateway
        .bind_lifecycle_timing_v2(lifecycle_timing.clone());
    if !startup_budget.operation_is_open() {
        drop(closed_components);
        return Err(cleanup_after_operation_deadline_v1(databases, &startup_budget).await);
    }
    let interaction_dispatch_port = databases
        .compose_interaction_dispatch_port_v1(
            closed_components
                .registry
                .interaction_dispatch_registry_v1(),
            secrets.discord_bot_token(),
            config.gateway(),
            startup_budget.operation_cutoff(),
        )
        .await;
    let interaction_dispatch_port = match interaction_dispatch_port {
        Ok(port) => port,
        Err(composition) => {
            drop(closed_components);
            let shutdown = databases.shutdown();
            drop(databases);
            return match shutdown
                .close_until(startup_budget.cleanup_deadline())
                .await
            {
                Ok(()) => Err(
                    RuntimeProcessFoundationCompositionErrorV1::InteractionDispatch(composition),
                ),
                Err(cleanup) => Err(
                    RuntimeProcessFoundationCompositionErrorV1::CleanupAfterInteractionDispatch {
                        composition,
                        cleanup,
                    },
                ),
            };
        }
    };
    if !startup_budget.operation_is_open() {
        drop((closed_components, interaction_dispatch_port));
        return Err(cleanup_after_operation_deadline_v1(databases, &startup_budget).await);
    }
    let finalizer_generation =
        RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::MIN).expect("finalizer generation");
    let mutation_finalizer =
        match certification_finalizer::RuntimeProcessMutationFinalizerSupervisorV3::start(
            pending_drain_finalizer::production_finalizer_config_v3(),
            finalizer_generation,
            certification_finalizer::RuntimeProcessMutationFinalizerPortV3::new(),
        ) {
            Ok(finalizer) => finalizer,
            Err(composition) => {
                drop((closed_components, interaction_dispatch_port));
                let shutdown = databases.shutdown();
                drop(databases);
                return match shutdown
                    .close_until(startup_budget.cleanup_deadline())
                    .await
                {
                    Ok(()) => Err(
                        RuntimeProcessFoundationCompositionErrorV1::MutationFinalizer(composition),
                    ),
                    Err(cleanup) => Err(
                        RuntimeProcessFoundationCompositionErrorV1::CleanupAfterMutationFinalizer {
                            composition,
                            cleanup,
                        },
                    ),
                };
            }
        };
    let (maintenance_ingress, maintenance_ingress_observer, maintenance_ingress_shutdown) =
        RuntimeMaintenanceIngressGateControllerV2::new_v2();
    let ingress_acknowledgement = RuntimeProcessIngressAcknowledgementSupervisorV2::start(
        databases.execution().clone(),
        RuntimeIngressAcknowledgementSupervisorConfigV2::new(std::time::Duration::from_millis(25))
            .expect("nonzero ingress acknowledgement retry delay"),
    );
    let root_supervisor = match RuntimeProcessRootSupervisorV1::start(
        config.health_bind_addr(),
        mutation_finalizer.seal_handle(),
        mutation_finalizer.terminal_observer(),
        closed_components.gateway.shutdown_handle_v1(),
        RuntimeProcessRootSupervisorControlV2::new_v2(
            maintenance_ingress_shutdown,
            ingress_acknowledgement.shutdown_handle_v2(),
            ingress_acknowledgement.terminal_observer_v2(),
            lifecycle_timing.clone(),
        ),
    )
    .await
    {
        Ok(supervisor) => supervisor,
        Err(composition) => {
            let ingress_acknowledgement_report = ingress_acknowledgement
                .shutdown_until(startup_budget.cleanup_deadline())
                .await;
            drop(ingress_acknowledgement_report);
            let finalizer_report = mutation_finalizer
                .shutdown_until(startup_budget.cleanup_deadline())
                .await;
            drop(finalizer_report);
            drop((closed_components, interaction_dispatch_port));
            let shutdown = databases.shutdown();
            drop(databases);
            let cleanup = shutdown
                .close_until(startup_budget.cleanup_deadline())
                .await;
            return match cleanup {
                Ok(()) => Err(map_root_supervisor_composition_error_v1(composition)),
                Err(cleanup) => Err(
                    RuntimeProcessFoundationCompositionErrorV1::CleanupAfterRootSupervisor {
                        cleanup,
                    },
                ),
            };
        }
    };
    let mut root_supervisor = root_supervisor;
    let readiness_publisher = root_supervisor
        .take_readiness_publisher_v2()
        .expect("fresh runtime process root supervisor");
    Ok(RuntimeProcessFoundationV1 {
        gateway: closed_components.gateway,
        registry: closed_components.registry,
        interaction_dispatch_port,
        databases,
        secrets,
        config,
        startup_budget,
        build_revision,
        process_instance_id,
        controller_id,
        mutation_finalizer: Some(mutation_finalizer),
        process_mutation_finalizer: None,
        maintenance_ingress: Some(maintenance_ingress),
        maintenance_ingress_observer,
        readiness_publisher: Some(readiness_publisher),
        ingress_acknowledgement: Some(ingress_acknowledgement),
        lifecycle_timing,
        lifecycle_timing_terminal: Some(lifecycle_timing_terminal),
        cleanup_mode: RuntimeProcessFoundationCleanupModeV1::StartupBound,
        root_supervisor,
        shutdown_failures: RuntimeProcessFoundationShutdownFailuresV1::new(),
    })
}

fn map_root_supervisor_composition_error_v1(
    error: RuntimeProcessRootSupervisorStartErrorV1,
) -> RuntimeProcessFoundationCompositionErrorV1 {
    match error {
        RuntimeProcessRootSupervisorStartErrorV1::Signal(_) => {
            RuntimeProcessFoundationCompositionErrorV1::ShutdownSignal
        }
        RuntimeProcessRootSupervisorStartErrorV1::Health(_) => {
            RuntimeProcessFoundationCompositionErrorV1::HealthListener
        }
    }
}

fn accept_shutdown_serving_registry_observation_v2(
    observation: Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1>,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    observation.map(drop)
}

async fn cleanup_after_operation_deadline_v1(
    databases: RuntimeDatabaseDependenciesV1,
    startup_budget: &RuntimeStartupBudgetV1,
) -> RuntimeProcessFoundationCompositionErrorV1 {
    let shutdown = databases.shutdown();
    drop(databases);
    match shutdown
        .close_until(startup_budget.cleanup_deadline())
        .await
    {
        Ok(()) => RuntimeProcessFoundationCompositionErrorV1::OperationDeadlineElapsed,
        Err(cleanup) => {
            RuntimeProcessFoundationCompositionErrorV1::CleanupAfterOperationDeadline { cleanup }
        }
    }
}

struct RuntimeClosedProcessComponentsV1 {
    registry: RuntimeRegistryBootstrapV1,
    gateway: RuntimeGatewayBootstrapV1,
}

#[derive(Clone, Copy, Debug)]
enum RuntimeClosedProcessComponentsErrorV1 {
    Registry(RuntimeRegistryBootstrapErrorV1),
    Gateway(RuntimeGatewayBootstrapErrorV1),
}

impl RuntimeClosedProcessComponentsErrorV1 {
    fn into_public(
        self,
        cleanup: Option<RuntimeDatabasePoolShutdownErrorV1>,
    ) -> RuntimeProcessFoundationCompositionErrorV1 {
        match (self, cleanup) {
            (Self::Registry(composition), None) => {
                RuntimeProcessFoundationCompositionErrorV1::Registry(composition)
            }
            (Self::Gateway(composition), None) => {
                RuntimeProcessFoundationCompositionErrorV1::Gateway(composition)
            }
            (Self::Registry(composition), Some(cleanup)) => {
                RuntimeProcessFoundationCompositionErrorV1::CleanupAfterRegistry {
                    composition,
                    cleanup,
                }
            }
            (Self::Gateway(composition), Some(cleanup)) => {
                RuntimeProcessFoundationCompositionErrorV1::CleanupAfterGateway {
                    composition,
                    cleanup,
                }
            }
        }
    }
}

fn compose_closed_process_components_v1(
    process_instance_id: &ProcessInstanceId,
    gateway_config: GatewayResourceConfigV1,
) -> Result<RuntimeClosedProcessComponentsV1, RuntimeClosedProcessComponentsErrorV1> {
    let registry =
        compose_runtime_registry_bootstrap_v1(process_instance_id.clone(), gateway_config)
            .map_err(RuntimeClosedProcessComponentsErrorV1::Registry)?;
    let gateway = compose_runtime_gateway_bootstrap_v1(process_instance_id.clone(), gateway_config)
        .map_err(RuntimeClosedProcessComponentsErrorV1::Gateway)?;
    Ok(RuntimeClosedProcessComponentsV1 { registry, gateway })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use automation_runtime_worker::{
        accept_runtime_route_set_observation_v2, RuntimeGatewayClosedSnapshotV2,
        RuntimeGatewayEmergencyCauseV2, RuntimeRegistryGlobalObservationSequenceV2,
        RuntimeRegistryRecoveryObservationInputV2, RuntimeRouteSetObservationInputV2,
    };

    use super::*;
    use crate::RuntimeGatewayReadyObservationErrorV1;

    fn process_instance_id() -> ProcessInstanceId {
        ProcessInstanceId::parse("runtime-process:foundation").unwrap()
    }

    struct DropProbeV1 {
        name: &'static str,
        events: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Drop for DropProbeV1 {
        fn drop(&mut self) {
            self.events.borrow_mut().push(self.name);
        }
    }

    fn assert_shutdown_finish_drop_order(result: Result<(), RuntimeDatabasePoolShutdownErrorV1>) {
        let events = Rc::new(RefCell::new(vec!["pool_close_returned"]));
        let returned = finish_runtime_process_foundation_shutdown_v1(
            DropProbeV1 {
                name: "secrets",
                events: events.clone(),
            },
            DropProbeV1 {
                name: "retained",
                events: events.clone(),
            },
            result,
        );

        assert_eq!(returned, result);
        assert_eq!(
            events.borrow().as_slice(),
            ["pool_close_returned", "secrets", "retained"]
        );
    }

    #[test]
    fn closed_components_bind_one_identity_and_expose_no_ready_gateway() {
        let components = compose_closed_process_components_v1(
            &process_instance_id(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let registry = components
            .registry
            .observe_recovery_empty_projection_v2()
            .unwrap();

        assert_eq!(
            registry.process_instance_id().as_str(),
            "runtime-process:foundation"
        );
        assert_eq!(registry.retained_slot_count(), 0);
        assert!(matches!(
            components.gateway.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
                ..
            }
        ));
        assert_eq!(
            components.gateway.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        assert_eq!(
            components.gateway.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::NotConnected)
        );
    }

    #[test]
    fn shutdown_finish_drops_secrets_only_after_close_returns_on_every_result() {
        assert_shutdown_finish_drop_order(Ok(()));
        assert_shutdown_finish_drop_order(Err(RuntimeDatabasePoolShutdownErrorV1::TimedOut));
    }

    #[test]
    fn serving_shutdown_accepts_nonempty_route_sets_and_preserves_registry_errors() {
        let observation =
            accept_runtime_route_set_observation_v2(RuntimeRouteSetObservationInputV2 {
                process_instance_id: process_instance_id(),
                registry: RuntimeRegistryRecoveryObservationInputV2 {
                    observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                        NonZeroU64::MIN,
                    ),
                    retained_slot_count: 1,
                    retained_empty_tombstone_count: 0,
                    staged_route_count: 0,
                    serving_route_count: 1,
                    draining_route_count: 0,
                    sealed_slot_count: 0,
                    active_interaction_count: 1,
                    failed_closed_slot_count: 0,
                    registry_failed_closed: false,
                },
            })
            .unwrap();

        assert!(!observation.is_empty());
        assert_eq!(
            accept_shutdown_serving_registry_observation_v2(Ok(observation)),
            Ok(())
        );
        assert_eq!(
            accept_shutdown_serving_registry_observation_v2(Err(
                RuntimeRegistryRecoveryObservationErrorV1::RegistryUnavailable,
            )),
            Err(RuntimeRegistryRecoveryObservationErrorV1::RegistryUnavailable)
        );
    }

    #[test]
    fn public_errors_keep_stable_finite_codes_and_redacted_debug() {
        let database = RuntimeProcessFoundationCompositionErrorV1::Database(
            RuntimeDatabaseCompositionErrorV1::Unavailable {
                capability: crate::DatabaseCapabilityV1::Panel,
            },
        );
        let registry = RuntimeClosedProcessComponentsErrorV1::Registry(
            RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity,
        )
        .into_public(None);
        let gateway = RuntimeClosedProcessComponentsErrorV1::Gateway(
            RuntimeGatewayBootstrapErrorV1::CommandCapacity,
        )
        .into_public(Some(RuntimeDatabasePoolShutdownErrorV1::TimedOut));
        let database_cleanup = RuntimeProcessFoundationCompositionErrorV1::Database(
            RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut,
        );
        let process_instance_id = RuntimeProcessFoundationCompositionErrorV1::ProcessInstanceId(
            RuntimeProcessInstanceIdGenerationErrorV1::EntropyUnavailable,
        );
        let controller_id = RuntimeProcessFoundationCompositionErrorV1::ControllerId(
            RuntimeControllerIdGenerationErrorV1::EntropyUnavailable,
        );
        let interaction_dispatch = RuntimeProcessFoundationCompositionErrorV1::InteractionDispatch(
            RuntimeInteractionDispatchCompositionErrorV1::SnapshotUnavailable,
        );

        assert_eq!(
            process_instance_id.code(),
            "runtime_process_instance_id_entropy_unavailable"
        );
        assert_eq!(process_instance_id.context(), None);
        assert_eq!(
            controller_id.code(),
            "runtime_controller_id_entropy_unavailable"
        );
        assert_eq!(controller_id.context(), None);
        assert_eq!(database.code(), "runtime_database_unavailable");
        assert_eq!(database.context(), Some("panel"));
        assert_eq!(
            registry.code(),
            "runtime_registry_active_interaction_capacity"
        );
        assert_eq!(registry.context(), None);
        assert_eq!(
            gateway.code(),
            "runtime_process_foundation_cleanup_after_gateway"
        );
        assert_eq!(gateway.context(), None);
        assert_eq!(
            database_cleanup.code(),
            "runtime_database_startup_cleanup_timed_out"
        );
        assert_eq!(database_cleanup.context(), None);
        assert_eq!(
            interaction_dispatch.code(),
            "runtime_interaction_dispatch_snapshot_unavailable"
        );
        assert_eq!(interaction_dispatch.context(), None);
        let deadline = RuntimeProcessFoundationCompositionErrorV1::OperationDeadlineElapsed;
        let deadline_cleanup =
            RuntimeProcessFoundationCompositionErrorV1::CleanupAfterOperationDeadline {
                cleanup: RuntimeDatabasePoolShutdownErrorV1::TimedOut,
            };
        assert_eq!(
            deadline.code(),
            "runtime_process_foundation_operation_deadline_elapsed"
        );
        assert_eq!(
            deadline_cleanup.code(),
            "runtime_process_foundation_cleanup_after_operation_deadline"
        );
        assert_eq!(deadline.context(), None);
        assert_eq!(deadline_cleanup.context(), None);
        assert_eq!(
            format!("{gateway:?}"),
            "RuntimeProcessFoundationCompositionErrorV1(<redacted>)"
        );
    }
}
