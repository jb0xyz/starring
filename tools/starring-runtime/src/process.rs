use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU64, NonZeroU8};

use automation_runtime_controller::RuntimeBuildRevisionV1;
use automation_runtime_convergence::{ControllerId, ProcessInstanceId};
use automation_runtime_worker::RuntimeMutationFinalizerGenerationV1;

use crate::build_revision::CompiledRuntimeBuildRevisionV1;
use crate::controller_identity::generate_runtime_controller_id_v1;
use crate::database::compose_runtime_database_dependencies_v1;
use crate::process_identity::generate_runtime_process_instance_id_v1;
use crate::process_supervisor::{
    RuntimeProcessRootSupervisorStartErrorV1, RuntimeProcessRootSupervisorV1,
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

mod closed;
pub(crate) mod connected;
mod execution;
#[cfg_attr(test, allow(dead_code))]
mod observation;
mod owner;
mod pending_drain_finalizer;
mod readiness;
mod recovery;
mod startup_loop;

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
pub use observation::{
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
            Self::MutationFinalizer(_) => "runtime_process_foundation_mutation_finalizer",
            Self::ShutdownSignal => "runtime_process_foundation_shutdown_signal",
            Self::HealthListener => "runtime_process_foundation_health_listener",
            Self::CleanupAfterRegistry { .. } => {
                "runtime_process_foundation_cleanup_after_registry"
            }
            Self::CleanupAfterGateway { .. } => "runtime_process_foundation_cleanup_after_gateway",
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
            | Self::MutationFinalizer(_)
            | Self::ShutdownSignal
            | Self::HealthListener
            | Self::CleanupAfterRegistry { .. }
            | Self::CleanupAfterGateway { .. }
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

pub(crate) struct RuntimeProcessFoundationV1 {
    gateway: RuntimeGatewayBootstrapV1,
    registry: RuntimeRegistryBootstrapV1,
    databases: RuntimeDatabaseDependenciesV1,
    secrets: ResolvedRuntimeSecretsV1,
    config: RuntimeConfigV1,
    startup_budget: RuntimeStartupBudgetV1,
    build_revision: RuntimeBuildRevisionV1,
    process_instance_id: ProcessInstanceId,
    controller_id: ControllerId,
    mutation_finalizer: Option<
        pending_drain_finalizer::RuntimePendingDrainFinalizerSupervisorV3<
            execution::RuntimeProductionPendingDrainFinalizerEnvironmentV3,
        >,
    >,
    root_supervisor: RuntimeProcessRootSupervisorV1,
    shutdown_failures: RuntimeProcessFoundationShutdownFailuresV1,
}

impl RuntimeProcessFoundationV1 {
    pub(super) fn shutdown_observer_v1(&self) -> crate::shutdown::RuntimeShutdownObserverV1 {
        self.root_supervisor.observer()
    }

    pub(super) fn shutdown_trigger_v1(
        &self,
    ) -> crate::process_supervisor::RuntimeProcessShutdownTriggerV1 {
        self.root_supervisor.shutdown_trigger()
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
        observation
            .deadline()
            .min(self.startup_budget.cleanup_deadline())
    }

    pub(super) async fn begin_shutdown_v1(
        &mut self,
        cause: RuntimeShutdownCauseV1,
    ) -> std::time::Instant {
        let observation = self.trip_shutdown_v1(cause);
        let deadline = self.effective_shutdown_deadline_v1(observation);
        if let Some(mutation_finalizer) = self.mutation_finalizer.take() {
            let finalizer_report = mutation_finalizer.shutdown_until(deadline).await;
            if finalizer_report.exit() != RuntimeSupervisorExitV1::Commanded {
                self.shutdown_failures.record(
                    RuntimeProcessFoundationShutdownFailureV1::Finalizer(finalizer_report.exit()),
                );
            }
            drop(finalizer_report);
        }
        deadline
    }

    pub(super) fn observe_shutdown_registry_v1(&mut self) {
        if let Err(error) = self.registry.observe_recovery_empty_projection_v2() {
            self.shutdown_failures
                .record(RuntimeProcessFoundationShutdownFailureV1::Registry(error));
        }
    }

    pub(super) fn record_finalizer_shutdown_exit_v1(&mut self, exit: RuntimeSupervisorExitV1) {
        if exit != RuntimeSupervisorExitV1::Commanded {
            self.shutdown_failures
                .record(RuntimeProcessFoundationShutdownFailureV1::Finalizer(exit));
        }
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), RuntimeProcessFoundationShutdownErrorV1> {
        let cleanup_deadline = self
            .begin_shutdown_v1(RuntimeShutdownCauseV1::Explicit)
            .await;
        self.observe_shutdown_registry_v1();
        self.finish_shutdown_v1(cleanup_deadline).await
    }

    pub(super) async fn finish_shutdown_v1(
        self,
        cleanup_deadline: std::time::Instant,
    ) -> Result<(), RuntimeProcessFoundationShutdownErrorV1> {
        let Self {
            gateway,
            registry,
            databases,
            secrets,
            config,
            startup_budget,
            build_revision,
            process_instance_id,
            controller_id,
            mutation_finalizer,
            mut root_supervisor,
            mut shutdown_failures,
        } = self;
        if let Some(mutation_finalizer) = mutation_finalizer {
            let finalizer_report = mutation_finalizer.shutdown_until(cleanup_deadline).await;
            if finalizer_report.exit() != RuntimeSupervisorExitV1::Commanded {
                shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::Finalizer(
                    finalizer_report.exit(),
                ));
            }
            drop(finalizer_report);
        }
        let signal_exit = root_supervisor.join_signal_until(cleanup_deadline).await;
        if matches!(
            signal_exit,
            crate::process_supervisor::RuntimeProcessSignalTaskExitV1::StreamClosed
                | crate::process_supervisor::RuntimeProcessSignalTaskExitV1::Panicked
        ) {
            shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::SignalSupervisor);
        }
        let shutdown = databases.shutdown();
        drop((gateway, registry, databases));
        if let Err(error) = shutdown.close_until(cleanup_deadline).await {
            shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::Database(error));
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
            ),
            (),
        );
        if root_supervisor
            .shutdown_health_until(cleanup_deadline)
            .await
            .is_err()
        {
            shutdown_failures.record(RuntimeProcessFoundationShutdownFailureV1::HealthListener);
        }
        shutdown_failures.finish()
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
    if !startup_budget.operation_is_open() {
        drop(closed_components);
        return Err(cleanup_after_operation_deadline_v1(databases, &startup_budget).await);
    }
    let finalizer_generation =
        RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::MIN).expect("finalizer generation");
    let mutation_finalizer =
        match pending_drain_finalizer::RuntimePendingDrainFinalizerSupervisorV3::start(
            pending_drain_finalizer::production_finalizer_config_v3(),
            finalizer_generation,
            pending_drain_finalizer::RuntimePendingDrainFinalizerPortV3::new(),
        ) {
            Ok(finalizer) => finalizer,
            Err(composition) => {
                drop(closed_components);
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
    let root_supervisor = match RuntimeProcessRootSupervisorV1::start(
        config.health_bind_addr(),
        mutation_finalizer.seal_handle(),
        mutation_finalizer.terminal_observer(),
        closed_components.gateway.shutdown_handle_v1(),
    )
    .await
    {
        Ok(supervisor) => supervisor,
        Err(composition) => {
            let finalizer_report = mutation_finalizer
                .shutdown_until(startup_budget.cleanup_deadline())
                .await;
            drop(finalizer_report);
            drop(closed_components);
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
    Ok(RuntimeProcessFoundationV1 {
        gateway: closed_components.gateway,
        registry: closed_components.registry,
        databases,
        secrets,
        config,
        startup_budget,
        build_revision,
        process_instance_id,
        controller_id,
        mutation_finalizer: Some(mutation_finalizer),
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
        RuntimeGatewayClosedSnapshotV2, RuntimeGatewayEmergencyCauseV2,
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
