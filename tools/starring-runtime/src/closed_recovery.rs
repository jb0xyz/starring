use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeRecoveryIdV2;

use crate::database::RuntimeDatabaseReadinessV1;
use crate::gateway::{
    RuntimeGatewayBootstrapV1, RuntimeGatewayRecoveryOwnerCommitErrorV2,
    RuntimeGatewayRecoverySectionErrorV2, RuntimeRecoveryPendingGatewayBindingV2,
};
use crate::gateway_owner_startup_watchdog::{
    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2, RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    RuntimeGatewayOwnerPreparedClosedRecoveryV2,
};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeClosedRecoveryCommitErrorV2 {
    #[error("runtime closed recovery owner commit gateway section failed")]
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime closed recovery owner commit registry binding failed")]
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    #[error("runtime closed recovery owner commit failed")]
    Owner(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2),
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

pub(crate) struct RuntimeClosedRecoverySessionV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
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

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "runtime fixed-point composition consumes the committed closed session"
        )
    )]
    pub(crate) async fn commit_owner_v2(
        self,
    ) -> Result<RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.commit_owner_with_post_commit_v2(|| {}).await
    }

    async fn commit_owner_with_post_commit_v2(
        self,
        post_commit: impl FnOnce(),
    ) -> Result<RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.revalidate_v2().map_err(map_begin_commit_error_v2)?;
        let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };
        let Self {
            owner,
            gateway,
            registry,
        } = self;
        let owner = gateway
            .commit_prepared_owner_v2(&authority, owner)
            .await
            .map_err(map_gateway_owner_commit_error_v2)?;
        post_commit();
        let session = RuntimeClosedRecoverySessionV2 {
            owner,
            gateway,
            registry,
        };
        session.revalidate_v2()?;
        Ok(session)
    }
}

impl RuntimeClosedRecoverySessionV2 {
    fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        let section = self
            .gateway
            .committed_pending_section_v2(&self.owner)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)?;
        let registry = self
            .registry
            .revalidate_empty_projection_v2(&section)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)?;
        section
            .validate_empty_registry_projection_v2(&registry)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)
    }
}

impl Debug for RuntimeClosedRecoveryPendingPhaseV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryPendingPhaseV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoverySessionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoverySessionV2(<redacted>)")
    }
}

fn map_begin_commit_error_v2(
    error: RuntimeClosedRecoveryBeginErrorV2,
) -> RuntimeClosedRecoveryCommitErrorV2 {
    match error {
        RuntimeClosedRecoveryBeginErrorV2::Gateway(error) => {
            RuntimeClosedRecoveryCommitErrorV2::Gateway(error)
        }
        RuntimeClosedRecoveryBeginErrorV2::Registry(error) => {
            RuntimeClosedRecoveryCommitErrorV2::Registry(error)
        }
    }
}

fn map_gateway_owner_commit_error_v2(
    error: RuntimeGatewayRecoveryOwnerCommitErrorV2,
) -> RuntimeClosedRecoveryCommitErrorV2 {
    match error {
        RuntimeGatewayRecoveryOwnerCommitErrorV2::Section(error) => {
            RuntimeClosedRecoveryCommitErrorV2::Gateway(error)
        }
        RuntimeGatewayRecoveryOwnerCommitErrorV2::Owner(error) => {
            RuntimeClosedRecoveryCommitErrorV2::Owner(error)
        }
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
    pub(crate) async fn commit_owner_after_test_hook_v2(
        self,
        post_commit: impl FnOnce(),
    ) -> Result<RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.commit_owner_with_post_commit_v2(post_commit).await
    }

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
