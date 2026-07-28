use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use automation_runtime_controller::RuntimeRecoveryIdV2;
use automation_runtime_worker::{
    RuntimeAcceptedStartupRecoveryExecutionOutcomeV2, RuntimeAuthorizedStartupRecoveryExecutionV2,
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimeDurablyAcknowledgedPendingDrainSuccessionV3,
    RuntimeDurablyAcknowledgedPendingDrainV2, RuntimePendingDrainCandidateV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainRegistryUnsealWitnessV2,
};
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
    RuntimeRegistryPendingDrainSealBindingV2, RuntimeRegistryPendingDrainSuccessionSealBindingV3,
    RuntimeRegistryRecoveryObservationErrorV1,
};

#[path = "startup_recovery_observation.rs"]
#[cfg_attr(test, allow(dead_code))]
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
    registry: RuntimeClosedRecoverySessionRegistryV2,
    operation_cutoff: Instant,
    readiness: RuntimeClosedRecoveryReadinessStateV2,
}

enum RuntimeClosedRecoverySessionRegistryV2 {
    Empty(RuntimeRegistryEmptyRecoveryBindingV2),
    PendingDrainSealed(Box<RuntimeRegistryPendingDrainSealBindingV2>),
    #[cfg_attr(not(test), allow(dead_code))]
    PendingDrainSuccessionSealed(Box<RuntimeRegistryPendingDrainSuccessionSealBindingV3>),
    Failed,
}

pub(crate) struct RuntimeClosedRecoveryReadyIterationV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: RuntimeRecoveryPendingGatewayBindingV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
    iteration: Option<RuntimeAuthorizedStartupRecoveryIterationV2>,
}

enum RuntimeClosedRecoveryReadinessStateV2 {
    Available,
    Failed,
    Ready(RuntimeAuthorizedStartupRecoveryIterationV2),
}

pub(crate) struct RuntimeClosedRecoveryFixedPointV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: RuntimeRecoveryPendingGatewayBindingV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
    proof: RuntimeStartupRecoveryFixedPointProofV2,
}

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

    #[cfg(test)]
    pub(crate) async fn commit_owner_v2(
        self,
    ) -> Result<RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.commit_owner_with_post_commit_v2(|| {}).await
    }

    #[cfg(test)]
    async fn commit_owner_with_post_commit_v2(
        mut self,
        post_commit: impl FnOnce(),
    ) -> Result<RuntimeClosedRecoverySessionV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.commit_owner_in_place_v2().await?;
        post_commit();
        let session = self.try_into_committed_session_v2().map_err(|_| {
            RuntimeClosedRecoveryCommitErrorV2::Owner(
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation,
            )
        })?;
        session.revalidate_v2()?;
        Ok(session)
    }

    pub(crate) fn commit_cutoff_v2(&self) -> Instant {
        self.operation_cutoff
            .min(self.owner.observation().safety_deadline())
    }

    pub(crate) async fn commit_owner_in_place_v2(
        &mut self,
    ) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        self.revalidate_v2().map_err(map_begin_commit_error_v2)?;
        let commit_cutoff = self.commit_cutoff_v2();
        if Instant::now() >= commit_cutoff {
            return Err(RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed);
        }
        let authority = RuntimeClosedRecoveryTransitionAuthorityV2 { _private: () };
        self.gateway
            .commit_prepared_owner_in_place_v2(&authority, &mut self.owner, commit_cutoff)
            .await
            .map_err(map_gateway_owner_commit_error_v2)
    }

    pub(crate) fn try_into_committed_session_v2(
        self,
    ) -> Result<RuntimeClosedRecoverySessionV2, Box<Self>> {
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
        } = self;
        let owner = match owner.try_into_committed_closed_recovery_v2() {
            Ok(owner) => owner,
            Err(owner) => {
                return Err(Box::new(Self {
                    owner: *owner,
                    gateway,
                    registry,
                    operation_cutoff,
                }));
            }
        };
        Ok(RuntimeClosedRecoverySessionV2 {
            owner,
            gateway,
            registry: RuntimeClosedRecoverySessionRegistryV2::Empty(registry),
            operation_cutoff,
            readiness: RuntimeClosedRecoveryReadinessStateV2::Available,
        })
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
    pub(crate) fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        match &self.registry {
            RuntimeClosedRecoverySessionRegistryV2::Empty(registry) => {
                revalidate_committed_recovery_v2(
                    &self.owner,
                    &self.gateway,
                    registry,
                    self.operation_cutoff,
                )
            }
            RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed(registry) => {
                revalidate_committed_pending_drain_sealed_v2(
                    &self.owner,
                    &self.gateway,
                    registry,
                    self.operation_cutoff,
                )
            }
            RuntimeClosedRecoverySessionRegistryV2::PendingDrainSuccessionSealed(registry) => {
                revalidate_committed_pending_drain_succession_sealed_v3(
                    &self.owner,
                    &self.gateway,
                    registry,
                    self.operation_cutoff,
                )
            }
            RuntimeClosedRecoverySessionRegistryV2::Failed => {
                Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ))
            }
        }
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
            readiness,
        } = self;
        drop((gateway, registry, operation_cutoff, readiness));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }

    pub(crate) fn readiness_cutoff_v2(&self) -> Instant {
        self.operation_cutoff
            .min(self.owner.observation().safety_deadline())
    }

    pub(crate) fn owner_safety_deadline_v2(&self) -> Instant {
        self.owner.observation().safety_deadline()
    }

    pub(crate) fn owner_terminal_status_v2(
        &self,
    ) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.owner.terminal_status_v2()
    }

    pub(crate) fn owner_terminal_observation_v2(
        &self,
    ) -> impl Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static {
        self.owner.terminal_observation_v2()
    }

    pub(crate) fn begin_startup_recovery_execution_v2(
        &mut self,
        continuation: RuntimeStartupRecoveryContinuationV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeClosedRecoveryCommitErrorV2>
    {
        if !matches!(
            &self.readiness,
            RuntimeClosedRecoveryReadinessStateV2::Available
        ) {
            self.gateway.invalidate_protocol_violation_v2();
            return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            ));
        }
        self.revalidate_v2()?;
        if !matches!(
            &self.registry,
            RuntimeClosedRecoverySessionRegistryV2::Empty(_)
        ) {
            self.gateway.invalidate_protocol_violation_v2();
            return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            ));
        }
        self.gateway
            .begin_startup_recovery_execution_v2(&self.owner, continuation)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)
    }

    pub(crate) fn complete_startup_recovery_execution_v2(
        &mut self,
        completed: RuntimeCompletedStartupRecoveryExecutionV2,
    ) -> Result<RuntimeAcceptedStartupRecoveryExecutionOutcomeV2, RuntimeClosedRecoveryCommitErrorV2>
    {
        if !matches!(
            &self.registry,
            RuntimeClosedRecoverySessionRegistryV2::Empty(_)
        ) {
            self.gateway.invalidate_protocol_violation_v2();
            return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            ));
        }
        let outcome = self
            .gateway
            .complete_startup_recovery_execution_v2(&self.owner, completed)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)?;
        self.revalidate_v2()?;
        Ok(outcome)
    }

    pub(crate) fn invalidate_startup_recovery_execution_v2(&self) {
        self.gateway.invalidate_capability_not_ready_v2();
    }

    pub(crate) fn seal_pending_drain_candidate_v2(
        &mut self,
        candidate: &RuntimePendingDrainCandidateV2,
    ) -> Result<RuntimePendingDrainRegistrySealWitnessV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.revalidate_v2()?;
        let registry = match std::mem::replace(
            &mut self.registry,
            RuntimeClosedRecoverySessionRegistryV2::Failed,
        ) {
            RuntimeClosedRecoverySessionRegistryV2::Empty(registry) => registry,
            registry => {
                self.registry = registry;
                self.gateway.invalidate_protocol_violation_v2();
                return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ));
            }
        };
        let (sealed, witness) = registry
            .into_pending_drain_seal_binding_v2(candidate)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)?;
        self.registry =
            RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed(Box::new(sealed));
        self.revalidate_v2()?;
        Ok(witness)
    }

    pub(crate) fn unseal_pending_drain_after_durable_ack_v2(
        &mut self,
        durable: &RuntimeDurablyAcknowledgedPendingDrainV2,
    ) -> Result<RuntimePendingDrainRegistryUnsealWitnessV2, RuntimeClosedRecoveryCommitErrorV2>
    {
        self.revalidate_v2()?;
        let registry = match std::mem::replace(
            &mut self.registry,
            RuntimeClosedRecoverySessionRegistryV2::Failed,
        ) {
            RuntimeClosedRecoverySessionRegistryV2::PendingDrainSealed(registry) => registry,
            registry => {
                self.registry = registry;
                self.gateway.invalidate_protocol_violation_v2();
                return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ));
            }
        };
        let (empty, witness) = (*registry)
            .into_empty_binding_after_durable_ack_v2(durable)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)?;
        self.registry = RuntimeClosedRecoverySessionRegistryV2::Empty(empty);
        Ok(witness)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn seal_pending_drain_succession_candidate_v3(
        &mut self,
        candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    ) -> Result<RuntimePendingDrainRegistrySealWitnessV2, RuntimeClosedRecoveryCommitErrorV2> {
        self.revalidate_v2()?;
        let registry = match std::mem::replace(
            &mut self.registry,
            RuntimeClosedRecoverySessionRegistryV2::Failed,
        ) {
            RuntimeClosedRecoverySessionRegistryV2::Empty(registry) => registry,
            registry => {
                self.registry = registry;
                self.gateway.invalidate_protocol_violation_v2();
                return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ));
            }
        };
        let (sealed, witness) = registry
            .into_pending_drain_succession_seal_binding_v3(candidate)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)?;
        self.registry =
            RuntimeClosedRecoverySessionRegistryV2::PendingDrainSuccessionSealed(Box::new(sealed));
        self.revalidate_v2()?;
        Ok(witness)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn unseal_pending_drain_after_durable_succession_v3(
        &mut self,
        durable: &RuntimeDurablyAcknowledgedPendingDrainSuccessionV3,
    ) -> Result<RuntimePendingDrainRegistryUnsealWitnessV2, RuntimeClosedRecoveryCommitErrorV2>
    {
        self.revalidate_v2()?;
        let registry = match std::mem::replace(
            &mut self.registry,
            RuntimeClosedRecoverySessionRegistryV2::Failed,
        ) {
            RuntimeClosedRecoverySessionRegistryV2::PendingDrainSuccessionSealed(registry) => {
                registry
            }
            registry => {
                self.registry = registry;
                self.gateway.invalidate_protocol_violation_v2();
                return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ));
            }
        };
        let (empty, witness) = (*registry)
            .into_empty_binding_after_durable_succession_v3(durable)
            .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)?;
        self.registry = RuntimeClosedRecoverySessionRegistryV2::Empty(empty);
        Ok(witness)
    }

    pub(crate) async fn refresh_iteration_readiness_in_place_v2(
        &mut self,
        databases: &RuntimeDatabaseDependenciesV1,
    ) -> Result<(), RuntimeClosedRecoveryReadinessRefreshErrorV2> {
        self.refresh_iteration_readiness_in_place_with_v2(
            |cutoff| databases.verify_readiness_refresh_until_v2(cutoff),
            || {},
        )
        .await
    }

    async fn refresh_iteration_readiness_in_place_with_v2<Verify, Verification, PostRefresh>(
        &mut self,
        verify: Verify,
        post_refresh: PostRefresh,
    ) -> Result<(), RuntimeClosedRecoveryReadinessRefreshErrorV2>
    where
        Verify: FnOnce(Instant) -> Verification,
        Verification: Future<
            Output = Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1>,
        >,
        PostRefresh: FnOnce(),
    {
        if !matches!(
            std::mem::replace(
                &mut self.readiness,
                RuntimeClosedRecoveryReadinessStateV2::Failed,
            ),
            RuntimeClosedRecoveryReadinessStateV2::Available
        ) {
            self.gateway.invalidate_protocol_violation_v2();
            return Err(RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            ));
        }
        self.revalidate_v2().map_err(map_commit_refresh_error_v2)?;
        let verification_cutoff = self.readiness_cutoff_v2();
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
        let iteration = self
            .gateway
            .refresh_readiness_in_place_v2(&self.owner, readiness.into_exact_capability_receipts())
            .map_err(RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway)?;
        post_refresh();
        self.revalidate_v2().map_err(map_commit_refresh_error_v2)?;
        self.readiness = RuntimeClosedRecoveryReadinessStateV2::Ready(iteration);
        Ok(())
    }

    pub(crate) fn try_into_ready_iteration_v2(
        self,
    ) -> Result<RuntimeClosedRecoveryReadyIterationV2, Box<Self>> {
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
            readiness,
        } = self;
        match readiness {
            RuntimeClosedRecoveryReadinessStateV2::Ready(iteration) => {
                let RuntimeClosedRecoverySessionRegistryV2::Empty(registry) = registry else {
                    gateway.invalidate_protocol_violation_v2();
                    return Err(Box::new(Self {
                        owner,
                        gateway,
                        registry,
                        operation_cutoff,
                        readiness: RuntimeClosedRecoveryReadinessStateV2::Ready(iteration),
                    }));
                };
                Ok(RuntimeClosedRecoveryReadyIterationV2 {
                    owner,
                    gateway,
                    registry,
                    operation_cutoff,
                    iteration: Some(iteration),
                })
            }
            readiness => Err(Box::new(Self {
                owner,
                gateway,
                registry,
                operation_cutoff,
                readiness,
            })),
        }
    }
}

impl RuntimeClosedRecoveryReadyIterationV2 {
    pub(crate) fn revalidate_v2(&self) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
        if self.iteration.is_none() {
            return Err(RuntimeClosedRecoveryCommitErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            ));
        }
        revalidate_committed_recovery_v2(
            &self.owner,
            &self.gateway,
            &self.registry,
            self.operation_cutoff,
        )
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn owner_terminal_status_v2(
        &self,
    ) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.owner.terminal_status_v2()
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn owner_safety_deadline_v2(&self) -> Instant {
        self.owner.observation().safety_deadline()
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn owner_terminal_observation_v2(
        &self,
    ) -> impl Future<Output = RuntimeGatewayOwnerStartupWatchdogExitV1> + Send + 'static {
        self.owner.terminal_observation_v2()
    }

    #[cfg_attr(test, allow(dead_code))]
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
            iteration,
        } = self;
        drop((gateway, registry, operation_cutoff, iteration));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }
}

#[cfg(test)]
#[test]
fn pending_drain_succession_session_surface_is_type_separated_v3() {
    let _seal = RuntimeClosedRecoverySessionV2::seal_pending_drain_succession_candidate_v3;
    let _unseal = RuntimeClosedRecoverySessionV2::unseal_pending_drain_after_durable_succession_v3;
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

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn owner_terminal_status_v2(
        &self,
    ) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        self.owner.terminal_status_v2()
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn owner_safety_deadline_v2(&self) -> Instant {
        self.owner.observation().safety_deadline()
    }

    #[cfg_attr(test, allow(dead_code))]
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
            proof,
        } = self;
        drop((gateway, registry, operation_cutoff, proof));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
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

fn revalidate_committed_pending_drain_sealed_v2(
    owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: &RuntimeRecoveryPendingGatewayBindingV2,
    registry: &RuntimeRegistryPendingDrainSealBindingV2,
    operation_cutoff: Instant,
) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
    if Instant::now() >= operation_cutoff {
        return Err(RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed);
    }
    let section = gateway
        .committed_pending_section_v2(owner)
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)?;
    drop(section);
    registry
        .revalidate_sealed_v2()
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)
}

fn revalidate_committed_pending_drain_succession_sealed_v3(
    owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: &RuntimeRecoveryPendingGatewayBindingV2,
    registry: &RuntimeRegistryPendingDrainSuccessionSealBindingV3,
    operation_cutoff: Instant,
) -> Result<(), RuntimeClosedRecoveryCommitErrorV2> {
    if Instant::now() >= operation_cutoff {
        return Err(RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed);
    }
    let section = gateway
        .committed_pending_section_v2(owner)
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Gateway)?;
    drop(section);
    registry
        .revalidate_sealed_v3()
        .map_err(RuntimeClosedRecoveryCommitErrorV2::Registry)
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

pub(crate) use startup_recovery_observation::{
    RuntimeClosedRecoveryStartupObservationAttemptErrorV2,
    RuntimeClosedRecoveryStartupObservationCleanupV2,
    RuntimeClosedRecoveryStartupObservationCompletionV2,
    RuntimeClosedRecoveryStartupObservationErrorV2,
};

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

    async fn refresh_iteration_readiness_with_v2<Verify, Verification, PostRefresh>(
        mut self,
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
        self.refresh_iteration_readiness_in_place_with_v2(verify, post_refresh)
            .await?;
        self.try_into_ready_iteration_v2().map_err(|_| {
            RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            )
        })
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

    pub(crate) async fn refresh_iteration_readiness_in_place_after_test_hook_v2<Verification>(
        &mut self,
        verification: Verification,
        post_refresh: impl FnOnce(),
    ) -> Result<(), RuntimeClosedRecoveryReadinessRefreshErrorV2>
    where
        Verification: Future<
            Output = Result<RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseCompositionErrorV1>,
        >,
    {
        self.refresh_iteration_readiness_in_place_with_v2(|_| verification, post_refresh)
            .await
    }

    #[cfg(test)]
    pub(crate) fn execute_startup_recovery_with_test_executor_v2<Execute>(
        mut self,
        continuation: RuntimeStartupRecoveryContinuationV2,
        execute: Execute,
    ) -> Result<
        (Self, RuntimeAcceptedStartupRecoveryExecutionOutcomeV2),
        RuntimeClosedRecoveryCommitErrorV2,
    >
    where
        Execute: FnOnce(
            RuntimeAuthorizedStartupRecoveryExecutionV2,
        ) -> RuntimeCompletedStartupRecoveryExecutionV2,
    {
        let authorization = self.begin_startup_recovery_execution_v2(continuation)?;
        let completed = execute(authorization);
        let outcome = self.complete_startup_recovery_execution_v2(completed)?;
        Ok((self, outcome))
    }
}

#[cfg(test)]
impl RuntimeClosedRecoveryReadyIterationV2 {
    pub(crate) fn with_operation_cutoff_for_test_v2(mut self, operation_cutoff: Instant) -> Self {
        self.operation_cutoff = operation_cutoff;
        self
    }
}
