use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeBuildRevisionV1;
use automation_runtime_convergence::{ControllerId, ProcessInstanceId};

use crate::controller_identity::generate_runtime_controller_id_v1;
use crate::database::compose_runtime_database_dependencies_v1;
use crate::process_identity::generate_runtime_process_instance_id_v1;
use crate::{
    compose_runtime_gateway_bootstrap_v1, compose_runtime_registry_bootstrap_v1,
    CompiledRuntimeBuildRevisionV1, GatewayResourceConfigV1, ResolvedRuntimeSecretsV1,
    RuntimeConfigV1, RuntimeControllerIdGenerationErrorV1, RuntimeDatabaseCompositionErrorV1,
    RuntimeDatabaseDependenciesV1, RuntimeDatabasePoolShutdownErrorV1,
    RuntimeGatewayBootstrapErrorV1, RuntimeGatewayBootstrapV1,
    RuntimeProcessInstanceIdGenerationErrorV1, RuntimeRegistryBootstrapErrorV1,
    RuntimeRegistryBootstrapV1, RuntimeStartupBudgetV1,
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
            Self::CleanupAfterRegistry { .. } => {
                "runtime_process_foundation_cleanup_after_registry"
            }
            Self::CleanupAfterGateway { .. } => "runtime_process_foundation_cleanup_after_gateway",
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
            | Self::CleanupAfterRegistry { .. }
            | Self::CleanupAfterGateway { .. }
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

pub struct RuntimeProcessFoundationV1 {
    startup_budget: RuntimeStartupBudgetV1,
    build_revision: CompiledRuntimeBuildRevisionV1,
    process_instance_id: ProcessInstanceId,
    controller_id: ControllerId,
    databases: RuntimeDatabaseDependenciesV1,
    registry: RuntimeRegistryBootstrapV1,
    gateway: RuntimeGatewayBootstrapV1,
}

impl RuntimeProcessFoundationV1 {
    pub fn runtime_build_revision(&self) -> &RuntimeBuildRevisionV1 {
        self.build_revision.revision()
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn controller_id(&self) -> &ControllerId {
        &self.controller_id
    }

    pub async fn shutdown(self) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
        let Self {
            startup_budget,
            build_revision,
            process_instance_id,
            controller_id,
            databases,
            registry,
            gateway,
        } = self;
        let cleanup_deadline = startup_budget.cleanup_deadline();
        let shutdown = databases.shutdown();
        drop((
            startup_budget,
            build_revision,
            process_instance_id,
            controller_id,
            databases,
            registry,
            gateway,
        ));
        shutdown.close_until(cleanup_deadline).await
    }
}

impl Debug for RuntimeProcessFoundationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationV1(<redacted>)")
    }
}

pub async fn compose_runtime_process_foundation_v1(
    startup_budget: RuntimeStartupBudgetV1,
    config: &RuntimeConfigV1,
    secrets: &ResolvedRuntimeSecretsV1,
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
    let databases = compose_runtime_database_dependencies_v1(config, secrets, &startup_budget)
        .await
        .map_err(RuntimeProcessFoundationCompositionErrorV1::Database)?;
    if !startup_budget.operation_is_open() {
        return Err(cleanup_after_operation_deadline_v1(databases, &startup_budget).await);
    }
    let closed_components =
        match compose_closed_process_components_v1(&process_instance_id, config.gateway()) {
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
    Ok(RuntimeProcessFoundationV1 {
        startup_budget,
        build_revision,
        process_instance_id,
        controller_id,
        databases,
        registry: closed_components.registry,
        gateway: closed_components.gateway,
    })
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
    use automation_runtime_worker::{
        RuntimeGatewayClosedSnapshotV2, RuntimeGatewayEmergencyCauseV2,
    };

    use super::*;
    use crate::RuntimeGatewayReadyObservationErrorV1;

    fn process_instance_id() -> ProcessInstanceId {
        ProcessInstanceId::parse("runtime-process:foundation").unwrap()
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
