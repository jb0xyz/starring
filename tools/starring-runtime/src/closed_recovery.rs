use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeRecoveryIdV2;

use crate::database::RuntimeDatabaseReadinessV1;
use crate::gateway::{
    RuntimeGatewayBootstrapV1, RuntimeGatewayRecoverySectionErrorV2,
    RuntimeRecoveryPendingGatewayBindingV2,
};
use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerPreparedClosedRecoveryV2;
use crate::registry::{
    RuntimeRegistryBootstrapV1, RuntimeRegistryEmptyRecoveryBindingV2,
    RuntimeRegistryRecoveryObservationErrorV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeClosedRecoveryBeginErrorV2 {
    #[error("runtime closed recovery gateway section failed")]
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime closed recovery registry binding failed")]
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
}

pub(crate) struct RuntimeClosedRecoveryTransitionAuthorityV2 {
    _private: (),
}

impl Debug for RuntimeClosedRecoveryTransitionAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryTransitionAuthorityV2(<redacted>)")
    }
}

pub(crate) struct RuntimeClosedRecoveryPendingPhaseV2 {
    owner: RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    gateway: RuntimeRecoveryPendingGatewayBindingV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
}

impl RuntimeClosedRecoveryPendingPhaseV2 {
    fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryBeginErrorV2> {
        let section = self
            .gateway
            .pending_section_v2(&self.owner)
            .map_err(RuntimeClosedRecoveryBeginErrorV2::Gateway)?;
        let registry = self
            .registry
            .revalidate_empty_projection_v2(&section)
            .map_err(RuntimeClosedRecoveryBeginErrorV2::Registry)?;
        section
            .validate_empty_registry_projection_v2(&registry)
            .map_err(RuntimeClosedRecoveryBeginErrorV2::Gateway)
    }
}

impl Debug for RuntimeClosedRecoveryPendingPhaseV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryPendingPhaseV2(<redacted>)")
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "owner commit composition consumes the exact pending phase"
    )
)]
pub(crate) fn begin_initial_empty_recovery_v2(
    gateway: &RuntimeGatewayBootstrapV1,
    registry: &RuntimeRegistryBootstrapV1,
    owner: RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    recovery_id: RuntimeRecoveryIdV2,
    readiness: &RuntimeDatabaseReadinessV1,
) -> Result<RuntimeClosedRecoveryPendingPhaseV2, RuntimeClosedRecoveryBeginErrorV2> {
    let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };
    let mut gateway_section = gateway
        .initial_emergency_gateway_section_v2(&owner)
        .map_err(|error| {
            RuntimeClosedRecoveryBeginErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::Gateway(error),
            )
        })?;
    let registry_guard = registry
        .recovery_observation_guard_v2(&authority, &gateway_section)
        .map_err(RuntimeClosedRecoveryBeginErrorV2::Registry)?;
    let registry_evidence = registry_guard
        .locked_empty_evidence_v2()
        .map_err(RuntimeClosedRecoveryBeginErrorV2::Registry)?;
    gateway_section
        .begin_empty_recovery_v2(
            &authority,
            recovery_id,
            readiness.exact_capability_receipts().clone(),
            registry_evidence,
        )
        .map_err(RuntimeClosedRecoveryBeginErrorV2::Gateway)?;
    let registry = registry_guard
        .into_empty_binding_v2()
        .map_err(RuntimeClosedRecoveryBeginErrorV2::Registry)?;
    let gateway = gateway_section
        .into_recovery_pending_binding_v2()
        .map_err(RuntimeClosedRecoveryBeginErrorV2::Gateway)?;
    let pending = RuntimeClosedRecoveryPendingPhaseV2 {
        owner,
        gateway,
        registry,
    };
    pending.revalidate_v2()?;
    Ok(pending)
}

#[cfg(test)]
impl RuntimeClosedRecoveryPendingPhaseV2 {
    pub(crate) fn stale_predecessor_drop_preserves_successor_v2(
        &mut self,
    ) -> Result<(), RuntimeClosedRecoveryBeginErrorV2> {
        let successor = self
            .gateway
            .successor_for_stale_drop_test_v2()
            .map_err(RuntimeClosedRecoveryBeginErrorV2::Gateway)?;
        let predecessor = std::mem::replace(&mut self.gateway, successor);
        drop(predecessor);
        self.revalidate_v2()
    }
}
