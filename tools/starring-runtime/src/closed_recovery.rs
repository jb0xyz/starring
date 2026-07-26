use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use automation_runtime_controller::RuntimeRecoveryIdV2;
use automation_runtime_worker::{
    RuntimeAuthorizedStartupRecoveryIterationV2, RuntimePausedGatewayObservationV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryFixedPointProofV2,
};

use crate::database::{
    RuntimeDatabaseCompositionErrorV1, RuntimeDatabaseDependenciesV1,
    RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseReadinessV1,
};
use crate::gateway::{
    RuntimeGatewayBootstrapV1, RuntimeGatewayRecoveryOwnerCommitErrorV2,
    RuntimeGatewayRecoverySectionErrorV2, RuntimeRecoveryPendingGatewayBindingV2,
};
use crate::gateway_owner_startup_watchdog::{
    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2, RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    RuntimeGatewayOwnerPreparedClosedRecoveryV2, RuntimeGatewayOwnerStartupWatchdogExitV1,
    RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
};
use crate::registry::{
    RuntimeRegistryBootstrapV1, RuntimeRegistryEmptyRecoveryBindingV2,
    RuntimeRegistryRecoveryObservationErrorV1,
};

#[path = "startup_recovery_observation.rs"]
mod startup_recovery_observation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeClosedRecoveryBeginErrorV2 {
    #[error("runtime closed recovery operation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime closed recovery gateway section failed")]
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime closed recovery registry binding failed")]
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeClosedRecoveryCommitErrorV2 {
    #[error("runtime closed recovery operation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime closed recovery owner commit gateway section failed")]
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime closed recovery owner commit registry binding failed")]
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    #[error("runtime closed recovery owner commit failed")]
    Owner(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeClosedRecoveryReadinessRefreshErrorV2 {
    #[error("runtime closed recovery readiness refresh deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime closed recovery readiness refresh database verification failed")]
    Database(RuntimeDatabaseCompositionErrorV1),
    #[error("runtime closed recovery readiness refresh gateway section failed")]
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime closed recovery readiness refresh registry binding failed")]
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    #[error("runtime closed recovery readiness refresh owner failed")]
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
    operation_cutoff: Instant,
}

pub(crate) struct RuntimeClosedRecoveryBeginFailureV2 {
    owner: Box<RuntimeGatewayOwnerPreparedClosedRecoveryV2>,
    error: RuntimeClosedRecoveryBeginErrorV2,
}

pub(crate) struct RuntimeClosedRecoverySessionV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: RuntimeRecoveryPendingGatewayBindingV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
}

pub(crate) struct RuntimeClosedRecoveryReadyIterationV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: RuntimeRecoveryPendingGatewayBindingV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
    iteration: RuntimeAuthorizedStartupRecoveryIterationV2,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "runtime resume composition will consume the verified fixed point"
    )
)]
pub(crate) struct RuntimeClosedRecoveryFixedPointV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: RuntimeRecoveryPendingGatewayBindingV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
    proof: RuntimeStartupRecoveryFixedPointProofV2,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "runtime recovery composition will consume the typed iteration outcome"
    )
)]
pub(crate) enum RuntimeClosedRecoveryStartupIterationOutcomeV2 {
    Continue {
        session: RuntimeClosedRecoverySessionV2,
        continuation: RuntimeStartupRecoveryContinuationV2,
    },
    FixedPoint(RuntimeClosedRecoveryFixedPointV2),
}

impl RuntimeClosedRecoveryPendingPhaseV2 {
    pub(crate) fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryBeginErrorV2> {
        if Instant::now() >= self.operation_cutoff {
            return Err(RuntimeClosedRecoveryBeginErrorV2::DeadlineElapsed);
        }
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

    pub(crate) async fn abort_and_shutdown_until_v2(
        self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
        } = self;
        drop((gateway, registry, operation_cutoff));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
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
        let commit_cutoff = self
            .operation_cutoff
            .min(self.owner.observation().safety_deadline());
        if Instant::now() >= commit_cutoff {
            return Err(RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed);
        }
        let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
        } = self;
        let owner = gateway
            .commit_prepared_owner_v2(&authority, owner, commit_cutoff)
            .await
            .map_err(map_gateway_owner_commit_error_v2)?;
        post_commit();
        let session = RuntimeClosedRecoverySessionV2 {
            owner,
            gateway,
            registry,
            operation_cutoff,
        };
        session.revalidate_v2()?;
        Ok(session)
    }
}

impl RuntimeClosedRecoveryBeginFailureV2 {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        RuntimeClosedRecoveryBeginErrorV2,
    ) {
        (*self.owner, self.error)
    }
}

impl RuntimeClosedRecoverySessionV2 {
    fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        revalidate_committed_recovery_v2(
            &self.owner,
            &self.gateway,
            &self.registry,
            self.operation_cutoff,
        )
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "runtime fixed-point loop refreshes the committed iteration authority"
        )
    )]
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) async fn refresh_iteration_readiness_v2(
        self,
        databases: &RuntimeDatabaseDependenciesV1,
    ) -> Result<RuntimeClosedRecoveryReadyIterationV2, RuntimeClosedRecoveryReadinessRefreshErrorV2>
    {
        self.refresh_iteration_readiness_with_v2(
            |cutoff| databases.verify_readiness_refresh_until_v2(cutoff),
            || {},
        )
        .await
    }

    async fn refresh_iteration_readiness_with_v2<Verify, Verification, PostRefresh>(
        self,
        verify: Verify,
        post_refresh: PostRefresh,
    ) -> Result<RuntimeClosedRecoveryReadyIterationV2, RuntimeClosedRecoveryReadinessRefreshErrorV2>
    where
        Verify: FnOnce(Instant) -> Verification,
        Verification: Future<
            Output = Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1>,
        >,
        PostRefresh: FnOnce(),
    {
        self.revalidate_v2().map_err(map_commit_refresh_error_v2)?;
        let verification_cutoff = self
            .operation_cutoff
            .min(self.owner.observation().safety_deadline());
        if Instant::now() >= verification_cutoff {
            return Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed);
        }
        let readiness = match verify(verification_cutoff).await {
            Ok(readiness) => readiness,
            Err(error) => {
                if Instant::now() >= verification_cutoff {
                    return Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed);
                }
                self.gateway.invalidate_capability_not_ready_v2();
                return Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::Database(
                    error,
                ));
            }
        };
        if Instant::now() >= verification_cutoff {
            return Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed);
        }
        self.revalidate_v2().map_err(map_commit_refresh_error_v2)?;
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
        } = self;
        let (gateway, iteration) = gateway
            .into_readiness_successor_v2(&owner, readiness.into_exact_capability_receipts())
            .map_err(RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway)?;
        post_refresh();
        let iteration = RuntimeClosedRecoveryReadyIterationV2 {
            owner,
            gateway,
            registry,
            operation_cutoff,
            iteration,
        };
        iteration
            .revalidate_v2()
            .map_err(map_commit_refresh_error_v2)?;
        Ok(iteration)
    }
}

impl RuntimeClosedRecoveryReadyIterationV2 {
    fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        revalidate_committed_recovery_v2(
            &self.owner,
            &self.gateway,
            &self.registry,
            self.operation_cutoff,
        )
    }
}

impl RuntimeClosedRecoveryFixedPointV2 {
    fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        revalidate_committed_recovery_v2(
            &self.owner,
            &self.gateway,
            &self.registry,
            self.operation_cutoff,
        )
    }

    #[cfg(test)]
    pub(crate) fn acknowledged_product_handoff_count_v2(&self) -> u32 {
        self.proof.acknowledged_product_handoff_count()
    }
}

fn revalidate_committed_recovery_v2(
    owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: &RuntimeRecoveryPendingGatewayBindingV2,
    registry: &RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
    if Instant::now() >= operation_cutoff {
        return Err(RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed);
    }
    let section = gateway
        .committed_pending_section_v2(owner)
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)?;
    let observation = registry
        .revalidate_empty_projection_v2(&section)
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)?;
    section
        .validate_empty_registry_projection_v2(&observation)
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)
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

impl Debug for RuntimeClosedRecoveryReadyIterationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryReadyIterationV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryFixedPointV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryFixedPointV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryStartupIterationOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryStartupIterationOutcomeV2(<redacted>)")
    }
}

fn map_begin_commit_error_v2(
    error: RuntimeClosedRecoveryBeginErrorV2,
) -> RuntimeClosedRecoveryCommitErrorV2 {
    match error {
        RuntimeClosedRecoveryBeginErrorV2::DeadlineElapsed => {
            RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed
        }
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
        RuntimeGatewayRecoveryOwnerCommitErrorV2::DeadlineElapsed => {
            RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed
        }
        RuntimeGatewayRecoveryOwnerCommitErrorV2::Section(error) => {
            RuntimeClosedRecoveryCommitErrorV2::Gateway(error)
        }
        RuntimeGatewayRecoveryOwnerCommitErrorV2::Owner(error) => {
            RuntimeClosedRecoveryCommitErrorV2::Owner(error)
        }
    }
}

fn map_commit_refresh_error_v2(
    error: RuntimeClosedRecoveryCommitErrorV2,
) -> RuntimeClosedRecoveryReadinessRefreshErrorV2 {
    match error {
        RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed => {
            RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed
        }
        RuntimeClosedRecoveryCommitErrorV2::Gateway(error) => {
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(error)
        }
        RuntimeClosedRecoveryCommitErrorV2::Registry(error) => {
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Registry(error)
        }
        RuntimeClosedRecoveryCommitErrorV2::Owner(error) => {
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Owner(error)
        }
    }
}

#[cfg(test)]
pub(crate) fn begin_initial_empty_recovery_v2(
    gateway: &RuntimeGatewayBootstrapV1,
    registry: &RuntimeRegistryBootstrapV1,
    owner: RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    recovery_id: RuntimeRecoveryIdV2,
    readiness: &RuntimeDatabaseReadinessV1,
    expected_paused_gateway: &RuntimePausedGatewayObservationV2,
    operation_cutoff: Instant,
) -> Result<RuntimeClosedRecoveryPendingPhaseV2, RuntimeClosedRecoveryBeginErrorV2> {
    begin_initial_empty_recovery_retained_v2(
        gateway,
        registry,
        owner,
        recovery_id,
        readiness,
        expected_paused_gateway,
        operation_cutoff,
    )
    .map_err(|failure| failure.error)
}

pub(crate) fn begin_initial_empty_recovery_retained_v2(
    gateway: &RuntimeGatewayBootstrapV1,
    registry: &RuntimeRegistryBootstrapV1,
    owner: RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    recovery_id: RuntimeRecoveryIdV2,
    readiness: &RuntimeDatabaseReadinessV1,
    expected_paused_gateway: &RuntimePausedGatewayObservationV2,
    operation_cutoff: Instant,
) -> Result<RuntimeClosedRecoveryPendingPhaseV2, RuntimeClosedRecoveryBeginFailureV2> {
    if Instant::now() >= operation_cutoff {
        return Err(RuntimeClosedRecoveryBeginFailureV2 {
            owner: Box::new(owner),
            error: RuntimeClosedRecoveryBeginErrorV2::DeadlineElapsed,
        });
    }
    let bindings = bind_initial_empty_recovery_v2(
        gateway,
        registry,
        &owner,
        recovery_id,
        readiness,
        expected_paused_gateway,
    );
    let (gateway, registry) = match bindings {
        Ok(bindings) => bindings,
        Err(error) => {
            return Err(RuntimeClosedRecoveryBeginFailureV2 {
                owner: Box::new(owner),
                error,
            });
        }
    };
    let pending = RuntimeClosedRecoveryPendingPhaseV2 {
        owner,
        gateway,
        registry,
        operation_cutoff,
    };
    if let Err(error) = pending.revalidate_v2() {
        let RuntimeClosedRecoveryPendingPhaseV2 {
            owner,
            gateway,
            registry,
            operation_cutoff,
        } = pending;
        drop((gateway, registry, operation_cutoff));
        return Err(RuntimeClosedRecoveryBeginFailureV2 {
            owner: Box::new(owner),
            error,
        });
    }
    Ok(pending)
}

fn bind_initial_empty_recovery_v2(
    gateway: &RuntimeGatewayBootstrapV1,
    registry: &RuntimeRegistryBootstrapV1,
    owner: &RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    recovery_id: RuntimeRecoveryIdV2,
    readiness: &RuntimeDatabaseReadinessV1,
    expected_paused_gateway: &RuntimePausedGatewayObservationV2,
) -> Result<
    (
        RuntimeRecoveryPendingGatewayBindingV2,
        RuntimeRegistryEmptyRecoveryBindingV2,
    ),
    RuntimeClosedRecoveryBeginErrorV2,
> {
    let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };
    let mut gateway_section = gateway
        .initial_emergency_gateway_section_v2(owner, expected_paused_gateway)
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
    Ok((gateway, registry))
}

#[cfg(test)]
pub(crate) use startup_recovery_observation::RuntimeClosedRecoveryStartupObservationErrorV2;

#[cfg(test)]
impl RuntimeClosedRecoveryPendingPhaseV2 {
    pub(crate) async fn commit_owner_after_test_hook_v2(
        self,
        post_commit: impl FnOnce(),
    ) -> Result<RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.commit_owner_with_post_commit_v2(post_commit).await
    }

    pub(crate) fn with_operation_cutoff_for_test_v2(mut self, operation_cutoff: Instant) -> Self {
        self.operation_cutoff = operation_cutoff;
        self
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

#[cfg(test)]
impl RuntimeClosedRecoverySessionV2 {
    pub(crate) fn with_operation_cutoff_for_test_v2(mut self, operation_cutoff: Instant) -> Self {
        self.operation_cutoff = operation_cutoff;
        self
    }

    pub(crate) async fn refresh_iteration_readiness_with_test_verifier_v2<Verify, Verification>(
        self,
        verify: Verify,
    ) -> Result<RuntimeClosedRecoveryReadyIterationV2, RuntimeClosedRecoveryReadinessRefreshErrorV2>
    where
        Verify: FnOnce(Instant) -> Verification,
        Verification: Future<
            Output = Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1>,
        >,
    {
        self.refresh_iteration_readiness_with_v2(verify, || {})
            .await
    }

    pub(crate) async fn refresh_iteration_readiness_after_test_hook_v2<Verification>(
        self,
        verification: Verification,
        post_refresh: impl FnOnce(),
    ) -> Result<RuntimeClosedRecoveryReadyIterationV2, RuntimeClosedRecoveryReadinessRefreshErrorV2>
    where
        Verification: Future<
            Output = Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1>,
        >,
    {
        self.refresh_iteration_readiness_with_v2(|_| verification, post_refresh)
            .await
    }
}
