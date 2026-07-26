use std::fmt::{Debug, Formatter};

use automation_runtime_convergence::ProcessInstanceId;

use crate::{
    compose_runtime_database_dependencies_v1, compose_runtime_gateway_bootstrap_v1,
    compose_runtime_registry_bootstrap_v1, GatewayResourceConfigV1, ResolvedRuntimeSecretsV1,
    RuntimeConfigV1, RuntimeDatabaseCompositionErrorV1, RuntimeDatabaseDependenciesV1,
    RuntimeDatabasePoolShutdownErrorV1, RuntimeGatewayBootstrapErrorV1, RuntimeGatewayBootstrapV1,
    RuntimeRegistryBootstrapErrorV1, RuntimeRegistryBootstrapV1,
};

#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProcessFoundationCompositionErrorV1 {
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
}

impl RuntimeProcessFoundationCompositionErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Database(error) => error.code(),
            Self::Registry(error) => error.code(),
            Self::Gateway(error) => error.code(),
            Self::CleanupAfterRegistry { .. } => {
                "runtime_process_foundation_cleanup_after_registry"
            }
            Self::CleanupAfterGateway { .. } => "runtime_process_foundation_cleanup_after_gateway",
        }
    }

    pub const fn context(self) -> Option<&'static str> {
        match self {
            Self::Database(error) => error.context(),
            Self::Registry(_)
            | Self::Gateway(_)
            | Self::CleanupAfterRegistry { .. }
            | Self::CleanupAfterGateway { .. } => None,
        }
    }
}

impl Debug for RuntimeProcessFoundationCompositionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationCompositionErrorV1(<redacted>)")
    }
}

pub struct RuntimeProcessFoundationV1 {
    process_instance_id: ProcessInstanceId,
    databases: RuntimeDatabaseDependenciesV1,
    registry: RuntimeRegistryBootstrapV1,
    gateway: RuntimeGatewayBootstrapV1,
}

impl RuntimeProcessFoundationV1 {
    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn databases(&self) -> &RuntimeDatabaseDependenciesV1 {
        &self.databases
    }

    pub fn registry(&self) -> &RuntimeRegistryBootstrapV1 {
        &self.registry
    }

    pub fn gateway(&self) -> &RuntimeGatewayBootstrapV1 {
        &self.gateway
    }

    pub async fn shutdown(self) -> Result<(), RuntimeDatabasePoolShutdownErrorV1> {
        let shutdown = self.databases.shutdown();
        drop(self);
        shutdown.close().await
    }
}

impl Debug for RuntimeProcessFoundationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProcessFoundationV1(<redacted>)")
    }
}

pub async fn compose_runtime_process_foundation_v1(
    config: &RuntimeConfigV1,
    secrets: &ResolvedRuntimeSecretsV1,
    process_instance_id: ProcessInstanceId,
) -> Result<RuntimeProcessFoundationV1, RuntimeProcessFoundationCompositionErrorV1> {
    let databases = compose_runtime_database_dependencies_v1(config, secrets)
        .await
        .map_err(RuntimeProcessFoundationCompositionErrorV1::Database)?;
    let closed_components =
        match compose_closed_process_components_v1(&process_instance_id, config.gateway()) {
            Ok(components) => components,
            Err(error) => {
                let shutdown = databases.shutdown();
                let cleanup = shutdown.close().await.err();
                return Err(error.into_public(cleanup));
            }
        };
    Ok(RuntimeProcessFoundationV1 {
        process_instance_id,
        databases,
        registry: closed_components.registry,
        gateway: closed_components.gateway,
    })
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
            format!("{gateway:?}"),
            "RuntimeProcessFoundationCompositionErrorV1(<redacted>)"
        );
    }
}
