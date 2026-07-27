use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeBuildRevisionV1;
use automation_runtime_convergence::{ControllerId, ProcessInstanceId};

use crate::build_revision::CompiledRuntimeBuildRevisionV1;
use crate::controller_identity::generate_runtime_controller_id_v1;
use crate::database::compose_runtime_database_dependencies_v1;
use crate::process_identity::generate_runtime_process_instance_id_v1;
use crate::startup::RuntimeStartupBudgetV1;
use crate::{
    compose_runtime_gateway_bootstrap_v1, compose_runtime_registry_bootstrap_v1,
    GatewayResourceConfigV1, ResolvedRuntimeSecretsV1, RuntimeConfigV1,
    RuntimeControllerIdGenerationErrorV1, RuntimeDatabaseCompositionErrorV1,
    RuntimeDatabaseDependenciesV1, RuntimeDatabasePoolShutdownErrorV1,
    RuntimeGatewayBootstrapErrorV1, RuntimeGatewayBootstrapV1,
    RuntimeProcessInstanceIdGenerationErrorV1, RuntimeRegistryBootstrapErrorV1,
    RuntimeRegistryBootstrapV1,
};

mod closed;
pub(crate) mod connected;
#[cfg_attr(test, allow(dead_code))]
mod observation;
mod owner;
mod readiness;
mod recovery;

pub use closed::{
    RuntimeClosedRecoveryProcessCleanupFailureV2, RuntimeClosedRecoveryProcessShutdownErrorV2,
    RuntimeProcessClosedRecoveryCommitFailureV2, RuntimeProcessClosedRecoveryTransitionErrorV2,
    RuntimeProcessClosedRecoveryTransitionFailureV2, RuntimeProcessGatewayOwnerCommitFailureV2,
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
pub use readiness::{
    RuntimeProcessRecoveryReadinessFailureV2, RuntimeProcessRecoveryReadinessTransitionErrorV2,
    RuntimeProcessRecoveryReadinessTransitionFailureV2,
    RuntimeRecoveryIterationReadyProcessShutdownErrorV2,
};
pub use recovery::{
    RuntimeProcessClosedRecoveryBeginFailureV2, RuntimeProcessGatewayOwnerPrepareFailureV2,
    RuntimeProcessRecoveryPendingTransitionErrorV2,
    RuntimeProcessRecoveryPendingTransitionFailureV2,
    RuntimeRecoveryPendingProcessCleanupFailureV2, RuntimeRecoveryPendingProcessShutdownErrorV2,
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
}

impl RuntimeProcessFoundationV1 {
    pub(crate) async fn shutdown(self) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
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
        } = self;
        let cleanup_deadline = startup_budget.cleanup_deadline();
        let shutdown = databases.shutdown();
        drop((gateway, registry, databases));
        let result = shutdown.close_until(cleanup_deadline).await;
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
            result,
        )
    }
}

fn finish_runtime_process_foundation_shutdown_v1<S, R>(
    secrets: S,
    retained: R,
    result: Result<(), RuntimeDatabasePoolShutdownErrorV1>,
) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
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
