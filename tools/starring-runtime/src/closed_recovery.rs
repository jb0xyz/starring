use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Instant;

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyAttestationV2,
    RuntimeIngressOpenAcknowledgementLeaseDurationV2, RuntimeRecoveryIdV2,
    RuntimeWriterFenceGenerationV1,
};
use automation_runtime_worker::{
    RuntimeAcceptedIngressOpenAcknowledgementV2, RuntimeAuthorizedStartupRecoveryIterationV2,
    RuntimeEmptyOpenAcknowledgementRefreshInputV2, RuntimeEmptyOpenAcknowledgementRefreshV2,
    RuntimeGatewayCoordinatorGenerationV2,
    RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2,
    RuntimeIngressOpenAcknowledgementPredecessorV2,
    RuntimeIngressOpenAcknowledgementSingleFlightV2, RuntimeOpenProductionObservationInputV2,
    RuntimeOpenProductionObservationPortV2, RuntimeOpenProductionObservationV2,
    RuntimeOpenProductionRequestV2, RuntimePausedGatewayObservationV2,
    RuntimeProductionHandoffObservationInputV2, RuntimeProductionHandoffObservationPortV2,
    RuntimeProductionHandoffObservationV2, RuntimeProductionHandoffProcessV2,
    RuntimeProductionLifecycleErrorV2, RuntimeRecoveryResumeObservationInputV2,
    RuntimeRecoveryResumeObservationV2, RuntimeRecoveryResumePortV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryFixedPointProofV2,
};
use automation_runtime_worker::{
    RuntimeAcceptedStartupRecoveryExecutionOutcomeV2, RuntimeAuthorizedStartupRecoveryExecutionV2,
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimeDurablyAcknowledgedPendingDrainSuccessionV3,
    RuntimeDurablyAcknowledgedPendingDrainV2, RuntimePendingDrainCandidateV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainRegistryUnsealWitnessV2,
};

use crate::database::{
    RuntimeDatabaseCompositionErrorV1, RuntimeDatabaseDependenciesV1,
    RuntimeDatabaseReadinessRefreshV2, RuntimeDatabaseReadinessV1,
};
use crate::discord_lifecycle::RuntimeDiscordPauseReservationIdentityV2;
use crate::gateway::{
    RuntimeGatewayBootstrapV1, RuntimeGatewayFixedPointAcceptanceErrorV2,
    RuntimeGatewayProductionCoordinatorV2, RuntimeGatewayProductionInterruptV2,
    RuntimeGatewayReadyInvalidationObserverV2, RuntimeGatewayRecoveryOwnerCommitErrorV2,
    RuntimeGatewayRecoverySectionErrorV2, RuntimeRecoveryPendingGatewayBindingV2,
};
use crate::gateway_owner_startup_watchdog::{
    RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2,
    RuntimeGatewayOwnerAdmissionFrozenSupervisorV2, RuntimeGatewayOwnerClosedRecoveryCommitErrorV2,
    RuntimeGatewayOwnerClosedRecoverySupervisorV2, RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    RuntimeGatewayOwnerProcessActivationErrorV2, RuntimeGatewayOwnerProcessFrozenSupervisorV2,
    RuntimeGatewayOwnerProcessRenewalStartErrorV2, RuntimeGatewayOwnerProductionSupervisorV2,
    RuntimeGatewayOwnerStartupWatchdogExitV1, RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    RuntimeGatewayOwnerStrictSuccessorErrorV2,
};
use crate::ingress_acknowledgement_supervisor::RuntimeIngressAcknowledgementAuthorityV2;
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

pub(crate) struct RuntimeGatewayOwnerAdmissionFrozenAuthorityV2 {
    baseline_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    cutoff: Instant,
}

impl RuntimeGatewayOwnerAdmissionFrozenAuthorityV2 {
    fn from_worker_fixed_point_v2(
        fixed_point: &automation_runtime_worker::RuntimeStartupRecoveryFixedPointProcessV2,
        cutoff: Instant,
    ) -> Option<Self> {
        let baseline_receipt = fixed_point.owner_receipt().clone();
        let minimum_database_now = fixed_point.minimum_database_now();
        if Instant::now() >= cutoff
            || baseline_receipt.database_now < minimum_database_now
            || baseline_receipt.database_lease_duration().is_none()
        {
            return None;
        }
        Some(Self {
            baseline_receipt,
            cutoff,
        })
    }

    pub(crate) fn cutoff_v2(&self) -> Instant {
        self.cutoff
    }

    #[cfg(test)]
    pub(crate) fn for_test_v2(
        baseline_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        cutoff: Instant,
    ) -> Self {
        Self {
            baseline_receipt,
            cutoff,
        }
    }

    pub(crate) fn accepts_current_v2(&self, current: &RuntimeGatewayOwnerLeaseReceiptV1) -> bool {
        current.lease_id == self.baseline_receipt.lease_id
            && current.owner_revision == self.baseline_receipt.owner_revision
            && current.expires_at == self.baseline_receipt.expires_at
            && current.database_now <= self.baseline_receipt.database_now
            && current.database_lease_duration().is_some()
    }

    pub(crate) fn accepts_observed_v2(&self, observed: &RuntimeGatewayOwnerLeaseReceiptV1) -> bool {
        observed.lease_id == self.baseline_receipt.lease_id
            && observed.owner_revision == self.baseline_receipt.owner_revision
            && observed.expires_at == self.baseline_receipt.expires_at
            && observed.database_now >= self.baseline_receipt.database_now
            && observed.database_lease_duration().is_some()
    }
}

impl Debug for RuntimeGatewayOwnerAdmissionFrozenAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayOwnerAdmissionFrozenAuthorityV2(<redacted>)")
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

pub(crate) struct RuntimeClosedRecoveryWorkerFixedPointV2 {
    owner: RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
    worker: automation_runtime_worker::RuntimeStartupRecoveryFixedPointProcessV2,
}

pub(crate) struct RuntimeClosedRecoveryAdmissionFrozenProcessV2 {
    owner: RuntimeGatewayOwnerAdmissionFrozenSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    operation_cutoff: Instant,
    worker: automation_runtime_worker::RuntimeStartupRecoveryFixedPointProcessV2,
}

pub(crate) struct RuntimeClosedRecoveryProcessFrozenProcessV2 {
    owner: RuntimeGatewayOwnerProcessFrozenSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    worker: automation_runtime_worker::RuntimeStartupRecoveryFixedPointProcessV2,
}

pub(crate) struct RuntimeClosedRecoveryProductionHandoffProcessV2 {
    owner: RuntimeGatewayOwnerProcessFrozenSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    worker: RuntimeProductionHandoffProcessV2,
}

pub(crate) struct RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2 {
    owner: RuntimeGatewayOwnerProcessFrozenSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    worker: automation_runtime_worker::RuntimeAdmissionAcknowledgingProcessV2,
}

pub(crate) struct RuntimeClosedRecoveryEmptyOpenProcessV2 {
    owner: RuntimeGatewayOwnerProcessFrozenSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    worker: automation_runtime_worker::RuntimeEmptyOpenProcessV2,
}

pub(crate) struct RuntimeClosedRecoverySupervisedEmptyOpenProcessV2 {
    owner: RuntimeGatewayOwnerProductionSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    worker: automation_runtime_worker::RuntimeEmptyOpenProcessV2,
}

pub(crate) struct RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2 {
    owner: RuntimeGatewayOwnerProductionSupervisorV2,
    gateway: RuntimeGatewayProductionCoordinatorV2,
    registry: RuntimeRegistryEmptyRecoveryBindingV2,
    worker: RuntimeEmptyOpenAcknowledgementRefreshV2,
}

pub(crate) struct RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2 {
    lifecycle: RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
    operation: RuntimeIngressOpenAcknowledgementSingleFlightV2,
}

pub(crate) enum RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2 {
    Admission(Box<RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2>),
    EmptyOpenRefresh(Box<RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2>),
}

pub(crate) enum RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2 {
    Admission {
        lifecycle: Box<RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2>,
        accepted: Box<RuntimeAcceptedIngressOpenAcknowledgementV2>,
    },
    EmptyOpenRefresh {
        lifecycle: Box<RuntimeClosedRecoverySupervisedEmptyOpenProcessV2>,
        accepted_receipt:
            Box<automation_runtime_controller::RuntimeIngressOpenAcknowledgementReceiptV2>,
    },
}

pub(crate) enum RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2 {
    Admission(Box<RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2>),
    EmptyOpenRefresh(Box<RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeClosedRecoveryIngressAcknowledgementCompletionErrorV2 {
    EmptyOpenRefresh(RuntimeProductionLifecycleErrorV2),
}

pub(crate) struct RuntimeClosedRecoveryAdmissionAcknowledgementAuthorizationFailureV2 {
    state: Box<RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2>,
    error: automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2,
}

impl Debug for RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2(<redacted>)")
    }
}

impl Debug for RuntimeClosedRecoveryAdmissionAcknowledgementAuthorizationFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "RuntimeClosedRecoveryAdmissionAcknowledgementAuthorizationFailureV2(<redacted>)",
        )
    }
}

pub(crate) struct RuntimeClosedRecoveryResumeObservationV2 {
    pub(crate) owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub(crate) readiness: automation_runtime_worker::RuntimeCapabilityReadinessSetV2,
    pub(crate) gateway_ready: RuntimeGatewayReadyAttestationV2,
    pub(crate) writer_fence_generation: RuntimeWriterFenceGenerationV1,
    pub(crate) maintenance_gate_generation:
        automation_runtime_worker::RuntimeMaintenanceGateGenerationV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeClosedRecoveryProductionHandoffErrorV2 {
    Owner,
    Gateway,
    Registry,
    Worker(RuntimeProductionLifecycleErrorV2),
}

pub(crate) struct RuntimeClosedRecoveryProductionHandoffFailureV2 {
    state: Box<RuntimeClosedRecoveryProcessFrozenProcessV2>,
    error: RuntimeClosedRecoveryProductionHandoffErrorV2,
}

pub(crate) struct RuntimeClosedRecoveryRecoveryResumeFailureV2 {
    state: Box<RuntimeClosedRecoveryProductionHandoffProcessV2>,
    error: RuntimeClosedRecoveryProductionHandoffErrorV2,
}

pub(crate) struct RuntimeClosedRecoveryEmptyOpenFailureV2 {
    state: Box<RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2>,
    error: RuntimeClosedRecoveryProductionHandoffErrorV2,
}

pub(crate) struct RuntimeClosedRecoveryProductionOwnerStartFailureV2 {
    state: Box<RuntimeClosedRecoveryEmptyOpenProcessV2>,
    error: RuntimeGatewayOwnerProcessRenewalStartErrorV2,
}

pub(crate) struct RuntimeClosedRecoveryAcknowledgementRefreshFailureV2 {
    state: Box<RuntimeClosedRecoverySupervisedEmptyOpenProcessV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeClosedRecoveryFixedPointHandoffErrorV2 {
    DeadlineElapsed,
    Recovery(RuntimeClosedRecoveryCommitErrorV2),
    Gateway(RuntimeGatewayFixedPointAcceptanceErrorV2),
    GatewayObservation(crate::RuntimeGatewayReadyObservationErrorV1),
    Registry(RuntimeRegistryRecoveryObservationErrorV1),
    Owner(RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2),
    OwnerProcess(RuntimeGatewayOwnerProcessActivationErrorV2),
    ProtocolViolation,
}

pub(crate) struct RuntimeClosedRecoveryFixedPointAcceptanceFailureV2 {
    fixed_point: Box<RuntimeClosedRecoveryFixedPointV2>,
    error: RuntimeClosedRecoveryFixedPointHandoffErrorV2,
}

pub(crate) struct RuntimeClosedRecoveryAdmissionFrozenFailureV2 {
    cleanup: RuntimeClosedRecoveryAdmissionFrozenCleanupV2,
    error: RuntimeClosedRecoveryFixedPointHandoffErrorV2,
}

enum RuntimeClosedRecoveryAdmissionFrozenCleanupV2 {
    Worker(Box<RuntimeClosedRecoveryWorkerFixedPointV2>),
    Frozen(Box<RuntimeClosedRecoveryAdmissionFrozenProcessV2>),
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

#[cfg(test)]
#[test]
fn ingress_acknowledgement_authority_surface_is_unified_v2() {
    fn assert_authority<T: RuntimeIngressAcknowledgementAuthorityV2>() {}

    assert_authority::<RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2>();
    let _initial =
        RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2::into_ingress_acknowledgement_authority_v2;
    let _refresh =
        RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2::into_ingress_acknowledgement_authority_v2;
    let _retained = RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2::into_retained_state_v2;
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

    pub(crate) fn handoff_cutoff_v2(&self) -> Instant {
        self.operation_cutoff
            .min(self.owner.observation().safety_deadline())
    }

    pub(crate) fn revalidate_for_handoff_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryFixedPointHandoffErrorV2> {
        self.revalidate_v2()
            .map_err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::Recovery)
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

    pub(crate) fn into_worker_fixed_point_v2(
        self,
    ) -> Result<
        RuntimeClosedRecoveryWorkerFixedPointV2,
        RuntimeClosedRecoveryFixedPointAcceptanceFailureV2,
    > {
        if let Err(error) = self.revalidate_v2() {
            return Err(RuntimeClosedRecoveryFixedPointAcceptanceFailureV2 {
                fixed_point: Box::new(self),
                error: RuntimeClosedRecoveryFixedPointHandoffErrorV2::Recovery(error),
            });
        }
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
            proof,
        } = self;
        match gateway.into_worker_fixed_point_v2(proof) {
            Ok((gateway, worker)) => Ok(RuntimeClosedRecoveryWorkerFixedPointV2 {
                owner,
                gateway,
                registry,
                operation_cutoff,
                worker,
            }),
            Err(failure) => {
                let (gateway, proof, error) = failure.into_parts();
                Err(RuntimeClosedRecoveryFixedPointAcceptanceFailureV2 {
                    fixed_point: Box::new(Self {
                        owner,
                        gateway,
                        registry,
                        operation_cutoff,
                        proof,
                    }),
                    error: RuntimeClosedRecoveryFixedPointHandoffErrorV2::Gateway(error),
                })
            }
        }
    }
}

impl RuntimeClosedRecoveryWorkerFixedPointV2 {
    pub(crate) async fn enter_admission_frozen_in_place_v2(
        &mut self,
    ) -> Result<(), RuntimeClosedRecoveryFixedPointHandoffErrorV2> {
        let cutoff = self
            .operation_cutoff
            .min(self.owner.observation().safety_deadline());
        let Some(authority) =
            RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::from_worker_fixed_point_v2(
                &self.worker,
                cutoff,
            )
        else {
            return Err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::DeadlineElapsed);
        };
        if let Some(interrupt) = self.gateway.current_interrupt_v2() {
            return Err(match interrupt {
                RuntimeGatewayProductionInterruptV2::Invalidation(_) => {
                    RuntimeClosedRecoveryFixedPointHandoffErrorV2::ProtocolViolation
                }
                RuntimeGatewayProductionInterruptV2::Shutdown => {
                    RuntimeClosedRecoveryFixedPointHandoffErrorV2::GatewayObservation(
                        crate::RuntimeGatewayReadyObservationErrorV1::Stopped,
                    )
                }
            });
        }
        if let Err(error) = self.gateway.revalidate_fixed_point_admission_v2() {
            return Err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::GatewayObservation(error));
        }
        if let Err(error) = self.registry.revalidate_production_empty_projection_v2() {
            return Err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::Registry(
                error,
            ));
        }
        if let Err(error) = self
            .owner
            .enter_admission_frozen_in_place_v2(authority)
            .await
        {
            return Err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::Owner(error));
        }
        Ok(())
    }

    pub(crate) fn try_into_admission_frozen_v2(
        self,
    ) -> Result<
        RuntimeClosedRecoveryAdmissionFrozenProcessV2,
        RuntimeClosedRecoveryAdmissionFrozenFailureV2,
    > {
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
            worker,
        } = self;
        let owner = match owner.try_into_admission_frozen_v2() {
            Ok(owner) => owner,
            Err(owner) => {
                return Err(RuntimeClosedRecoveryAdmissionFrozenFailureV2 {
                    cleanup: RuntimeClosedRecoveryAdmissionFrozenCleanupV2::Worker(Box::new(
                        Self {
                            owner: *owner,
                            gateway,
                            registry,
                            operation_cutoff,
                            worker,
                        },
                    )),
                    error: RuntimeClosedRecoveryFixedPointHandoffErrorV2::ProtocolViolation,
                });
            }
        };
        let frozen = RuntimeClosedRecoveryAdmissionFrozenProcessV2 {
            owner,
            gateway,
            registry,
            operation_cutoff,
            worker,
        };
        if let Err(error) = frozen.revalidate_paused_v2() {
            return Err(RuntimeClosedRecoveryAdmissionFrozenFailureV2 {
                cleanup: RuntimeClosedRecoveryAdmissionFrozenCleanupV2::Frozen(Box::new(frozen)),
                error,
            });
        }
        Ok(frozen)
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
            worker,
        } = self;
        drop((gateway, registry, operation_cutoff, worker));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }
}

impl RuntimeClosedRecoveryAdmissionFrozenProcessV2 {
    pub(crate) fn revalidate_paused_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryFixedPointHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.operation_cutoff <= Instant::now()
            || self.owner.handoff_cutoff_v2() <= Instant::now()
            || self.owner.handoff_observation_v2().safety_deadline() <= Instant::now()
            || self.gateway.current_interrupt_v2().is_some()
            || self.worker.coordinator_generation() != self.gateway.coordinator_generation_v2()
            || matches!(
                self.gateway.closed_snapshot_v2(),
                automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                    | automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
            )
        {
            return Err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::ProtocolViolation);
        }
        self.gateway
            .revalidate_fixed_point_admission_v2()
            .map_err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::GatewayObservation)?;
        self.registry
            .revalidate_production_empty_projection_v2()
            .map_err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::Registry)?;
        Ok(())
    }

    pub(crate) async fn activate_process_owner_in_place_v2(
        &mut self,
        process_generation: std::num::NonZeroU64,
    ) -> Result<(), RuntimeClosedRecoveryFixedPointHandoffErrorV2> {
        self.revalidate_paused_v2()?;
        self.owner
            .activate_process_ownership_in_place_v2(process_generation)
            .await
            .map_err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::OwnerProcess)?;
        Ok(())
    }

    pub(crate) fn try_into_process_frozen_v2(
        self,
    ) -> Result<RuntimeClosedRecoveryProcessFrozenProcessV2, Box<Self>> {
        let Self {
            owner,
            gateway,
            registry,
            operation_cutoff,
            worker,
        } = self;
        let owner = match owner.try_into_process_frozen_v2() {
            Ok(owner) => owner,
            Err(owner) => {
                return Err(Box::new(Self {
                    owner,
                    gateway,
                    registry,
                    operation_cutoff,
                    worker,
                }));
            }
        };
        let process = RuntimeClosedRecoveryProcessFrozenProcessV2 {
            owner,
            gateway,
            registry,
            worker,
        };
        Ok(process)
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
            worker,
        } = self;
        drop((gateway, registry, operation_cutoff, worker));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }
}

impl RuntimeClosedRecoveryProcessFrozenProcessV2 {
    pub(crate) fn process_generation_v2(&self) -> std::num::NonZeroU64 {
        self.owner.process_generation_v2()
    }

    pub(crate) async fn into_production_handoff_v2(
        self,
        finalizer_generation: automation_runtime_worker::RuntimeMutationFinalizerGenerationV1,
    ) -> Result<
        RuntimeClosedRecoveryProductionHandoffProcessV2,
        RuntimeClosedRecoveryProductionHandoffFailureV2,
    > {
        if self.revalidate_paused_v2().is_err() {
            return Err(RuntimeClosedRecoveryProductionHandoffFailureV2 {
                state: Box::new(self),
                error: RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway,
            });
        }
        let owner_receipt = match self.owner.observe_current_v2().await {
            Ok(observation)
                if observation.receipt() == self.owner.activation_observation_v2().receipt() =>
            {
                observation.receipt().clone()
            }
            Ok(_) | Err(_) => {
                return Err(RuntimeClosedRecoveryProductionHandoffFailureV2 {
                    state: Box::new(self),
                    error: RuntimeClosedRecoveryProductionHandoffErrorV2::Owner,
                });
            }
        };
        if self
            .registry
            .revalidate_production_empty_projection_v2()
            .is_err()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffFailureV2 {
                state: Box::new(self),
                error: RuntimeClosedRecoveryProductionHandoffErrorV2::Registry,
            });
        }
        if self.gateway.revalidate_fixed_point_admission_v2().is_err() {
            return Err(RuntimeClosedRecoveryProductionHandoffFailureV2 {
                state: Box::new(self),
                error: RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway,
            });
        }
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        let observer = RuntimeClosedRecoveryProductionHandoffObserverV2 {
            owner_receipt,
            finalizer_generation,
        };
        let worker = match worker.begin_production_handoff(&observer) {
            Ok(worker) => worker,
            Err(failure) => {
                let error = failure
                    .contract_error()
                    .unwrap_or(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady);
                return Err(RuntimeClosedRecoveryProductionHandoffFailureV2 {
                    state: Box::new(RuntimeClosedRecoveryProcessFrozenProcessV2 {
                        owner,
                        gateway,
                        registry,
                        worker: failure.into_state(),
                    }),
                    error: RuntimeClosedRecoveryProductionHandoffErrorV2::Worker(error),
                });
            }
        };
        let process = RuntimeClosedRecoveryProductionHandoffProcessV2 {
            owner,
            gateway,
            registry,
            worker,
        };
        Ok(process)
    }

    pub(crate) fn revalidate_paused_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryFixedPointHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.owner.activation_observation_v2().safety_deadline() <= Instant::now()
            || self.gateway.current_interrupt_v2().is_some()
            || self.worker.coordinator_generation() != self.gateway.coordinator_generation_v2()
            || matches!(
                self.gateway.closed_snapshot_v2(),
                automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                    | automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
            )
        {
            return Err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::ProtocolViolation);
        }
        self.gateway
            .revalidate_fixed_point_admission_v2()
            .map_err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::GatewayObservation)?;
        self.registry
            .revalidate_production_empty_projection_v2()
            .map_err(RuntimeClosedRecoveryFixedPointHandoffErrorV2::Registry)?;
        Ok(())
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
            worker,
        } = self;
        drop((gateway, registry, worker));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }
}

struct RuntimeClosedRecoveryProductionHandoffObserverV2 {
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    finalizer_generation: automation_runtime_worker::RuntimeMutationFinalizerGenerationV1,
}

impl RuntimeProductionHandoffObservationPortV2
    for RuntimeClosedRecoveryProductionHandoffObserverV2
{
    type Error = std::convert::Infallible;

    fn observe_production_handoff(
        &self,
        request: &automation_runtime_worker::RuntimeProductionHandoffRequestV2,
    ) -> Result<RuntimeProductionHandoffObservationV2, Self::Error> {
        Ok(RuntimeProductionHandoffObservationV2::new(
            RuntimeProductionHandoffObservationInputV2 {
                coordinator_generation: request.coordinator_generation(),
                recovery_id: request.recovery_id().clone(),
                recovery_authority_revision: request.recovery_authority_revision(),
                owner_receipt: self.owner_receipt.clone(),
                process_instance_id: request.process_instance_id().clone(),
                connection_epoch: request.connection_epoch(),
                paused_admission_revision: request.paused_admission_revision(),
                connected_event_sequence: request.connected_event_sequence(),
                pause_sequence: request.pause_sequence(),
                registry_observation_sequence: request.registry_observation_sequence(),
                finalizer_generation: self.finalizer_generation,
                startup_intake_sealed: true,
                startup_jobs_settled: true,
                supervisors_started: true,
            },
        ))
    }
}

impl RuntimeClosedRecoveryProductionHandoffProcessV2 {
    pub(crate) fn coordinator_generation_v2(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.gateway.coordinator_generation_v2()
    }

    pub(crate) fn observe_exact_pause_reservation_v2(
        &self,
    ) -> Result<
        RuntimeDiscordPauseReservationIdentityV2,
        crate::RuntimeGatewayReadyObservationErrorV1,
    > {
        self.gateway
            .observe_exact_pause_reservation_v2(self.worker.coordinator_generation())
    }

    pub(crate) fn recovery_resume_successor_generation_v2(
        &self,
    ) -> Result<RuntimeGatewayCoordinatorGenerationV2, crate::RuntimeGatewayReadyObservationErrorV1>
    {
        self.gateway
            .recovery_resume_successor_generation_v2(self.worker.coordinator_generation())
    }

    pub(crate) fn observe_exact_resumed_ready_attestation_v2(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, crate::RuntimeGatewayReadyObservationErrorV1>
    {
        let successor = self.recovery_resume_successor_generation_v2()?;
        self.gateway
            .observe_exact_current_ready_attestation_v2(successor)
    }

    pub(crate) fn revalidate_paused_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryProductionHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.gateway.current_interrupt_v2().is_some()
            || self.worker.coordinator_generation() != self.gateway.coordinator_generation_v2()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway);
        }
        self.gateway
            .revalidate_fixed_point_admission_v2()
            .map_err(|_| RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway)?;
        self.registry
            .revalidate_production_empty_projection_v2()
            .map_err(|_| RuntimeClosedRecoveryProductionHandoffErrorV2::Registry)?;
        Ok(())
    }

    fn revalidate_resumed_v2(&self) -> Result<(), RuntimeClosedRecoveryProductionHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.gateway.current_interrupt_v2().is_some()
            || self.observe_exact_resumed_ready_attestation_v2().is_err()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway);
        }
        self.registry
            .revalidate_production_empty_projection_v2()
            .map_err(|_| RuntimeClosedRecoveryProductionHandoffErrorV2::Registry)?;
        Ok(())
    }

    pub(crate) fn recovery_resume_permit_v2(
        &self,
    ) -> &automation_runtime_worker::RuntimeRecoveryResumePermitV2 {
        self.worker.recovery_resume_permit()
    }

    pub(crate) async fn into_admission_acknowledging_v2(
        self,
        observation: RuntimeClosedRecoveryResumeObservationV2,
    ) -> Result<
        RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2,
        RuntimeClosedRecoveryRecoveryResumeFailureV2,
    > {
        if self.revalidate_resumed_v2().is_err() {
            return Err(RuntimeClosedRecoveryRecoveryResumeFailureV2 {
                state: Box::new(self),
                error: RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway,
            });
        }
        let current_owner = match self.owner.observe_current_v2().await {
            Ok(current) => current,
            Err(_) => {
                return Err(RuntimeClosedRecoveryRecoveryResumeFailureV2 {
                    state: Box::new(self),
                    error: RuntimeClosedRecoveryProductionHandoffErrorV2::Owner,
                });
            }
        };
        let current_ready = match self.observe_exact_resumed_ready_attestation_v2() {
            Ok(ready) => ready,
            Err(_) => {
                return Err(RuntimeClosedRecoveryRecoveryResumeFailureV2 {
                    state: Box::new(self),
                    error: RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway,
                });
            }
        };
        if current_owner.receipt() != &observation.owner_receipt
            || current_ready != observation.gateway_ready
            || self
                .registry
                .revalidate_production_empty_projection_v2()
                .is_err()
        {
            return Err(RuntimeClosedRecoveryRecoveryResumeFailureV2 {
                state: Box::new(self),
                error: RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway,
            });
        }
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        let port = RuntimeClosedRecoveryResumeObserverV2 { observation };
        let worker = match worker.resume_recovery(&port) {
            Ok(worker) => worker,
            Err(failure) => {
                let error = failure
                    .contract_error()
                    .unwrap_or(RuntimeProductionLifecycleErrorV2::ResumePermitMismatch);
                return Err(RuntimeClosedRecoveryRecoveryResumeFailureV2 {
                    state: Box::new(RuntimeClosedRecoveryProductionHandoffProcessV2 {
                        owner,
                        gateway,
                        registry,
                        worker: failure.into_state(),
                    }),
                    error: RuntimeClosedRecoveryProductionHandoffErrorV2::Worker(error),
                });
            }
        };
        Ok(RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2 {
            owner,
            gateway,
            registry,
            worker,
        })
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
            worker,
        } = self;
        drop((gateway, registry, worker));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }
}

struct RuntimeClosedRecoveryResumeObserverV2 {
    observation: RuntimeClosedRecoveryResumeObservationV2,
}

impl RuntimeRecoveryResumePortV2 for RuntimeClosedRecoveryResumeObserverV2 {
    type Error = std::convert::Infallible;

    fn resume_or_observe_recovery(
        &self,
        permit: &automation_runtime_worker::RuntimeRecoveryResumePermitV2,
    ) -> Result<RuntimeRecoveryResumeObservationV2, Self::Error> {
        Ok(RuntimeRecoveryResumeObservationV2::new(
            RuntimeRecoveryResumeObservationInputV2 {
                coordinator_generation: permit.coordinator_generation(),
                recovery_id: permit.recovery_id().clone(),
                recovery_authority_revision: permit.recovery_authority_revision(),
                process_instance_id: permit.process_instance_id().clone(),
                connection_epoch: permit.connection_epoch(),
                paused_admission_revision: permit.paused_admission_revision(),
                connected_event_sequence: permit.connected_event_sequence(),
                pause_sequence: permit.pause_sequence(),
                owner_receipt: self.observation.owner_receipt.clone(),
                readiness: self.observation.readiness.clone(),
                registry_observation_sequence: permit.registry_observation_sequence(),
                finalizer_generation: permit.finalizer_generation(),
                writer_fence_generation: self.observation.writer_fence_generation,
                writer_fence_open: true,
                maintenance_gate_generation: self.observation.maintenance_gate_generation,
                maintenance_gate_closed: true,
                gateway_ready: self.observation.gateway_ready.clone(),
            },
        ))
    }
}

impl RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2 {
    pub(crate) fn process_generation_v2(&self) -> std::num::NonZeroU64 {
        self.owner.process_generation_v2()
    }

    pub(crate) fn coordinator_generation_v2(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.worker.coordinator_generation()
    }

    pub(crate) fn finalizer_generation_v2(
        &self,
    ) -> automation_runtime_worker::RuntimeMutationFinalizerGenerationV1 {
        self.worker.finalizer_generation()
    }

    pub(crate) fn authorize_ingress_acknowledgement_predecessor_observation_v2(
        &self,
    ) -> RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2 {
        self.worker
            .authorize_ingress_open_acknowledgement_predecessor_observation()
    }

    pub(crate) fn into_ingress_acknowledgement_authority_v2(
        self,
        open_maintenance_gate_generation:
            automation_runtime_worker::RuntimeMaintenanceGateGenerationV2,
        predecessor: RuntimeIngressOpenAcknowledgementPredecessorV2,
        lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2,
    ) -> Result<
        RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2,
        RuntimeClosedRecoveryAdmissionAcknowledgementAuthorizationFailureV2,
    > {
        let operation = match self.worker.authorize_ingress_open_acknowledgement(
            open_maintenance_gate_generation,
            predecessor,
            lease_for,
        ) {
            Ok(operation) => operation,
            Err(error) => {
                return Err(
                    RuntimeClosedRecoveryAdmissionAcknowledgementAuthorizationFailureV2 {
                        state: Box::new(self),
                        error,
                    },
                );
            }
        };
        Ok(
            RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2::Admission(Box::new(
                RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2 {
                    lifecycle: self,
                    operation,
                },
            )),
        )
    }

    pub(crate) async fn observe_current_owner_v2(
        &self,
    ) -> Result<
        RuntimeGatewayOwnerLeaseReceiptV1,
        crate::RuntimeGatewayOwnerCurrentObservationErrorV1,
    > {
        self.owner
            .observe_current_v2()
            .await
            .map(|observation| observation.receipt().clone())
    }

    pub(crate) fn observe_exact_current_ready_attestation_v2(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, crate::RuntimeGatewayReadyObservationErrorV1>
    {
        self.gateway
            .observe_exact_current_ready_attestation_v2(self.gateway.coordinator_generation_v2())
    }

    pub(crate) fn observe_registry_empty_v2(
        &self,
    ) -> Result<
        automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2,
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.registry.revalidate_production_empty_projection_v2()
    }

    pub(crate) fn revalidate_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryProductionHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.gateway.current_interrupt_v2().is_some()
            || self.worker.coordinator_generation() != self.gateway.coordinator_generation_v2()
            || self
                .gateway
                .observe_exact_current_ready_attestation_v2(self.worker.coordinator_generation())
                .is_err()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway);
        }
        self.registry
            .revalidate_production_empty_projection_v2()
            .map_err(|_| RuntimeClosedRecoveryProductionHandoffErrorV2::Registry)?;
        Ok(())
    }

    pub(crate) fn into_empty_open_v2(
        self,
        observation: RuntimeOpenProductionObservationInputV2,
    ) -> Result<RuntimeClosedRecoveryEmptyOpenProcessV2, RuntimeClosedRecoveryEmptyOpenFailureV2>
    {
        if self.revalidate_v2().is_err() {
            return Err(RuntimeClosedRecoveryEmptyOpenFailureV2 {
                state: Box::new(self),
                error: RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway,
            });
        }
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        let port = RuntimeClosedRecoveryOpenProductionObserverV2 {
            observation: std::cell::RefCell::new(Some(observation)),
        };
        let worker = match worker.observe_open_production(&port) {
            Ok(worker) => worker,
            Err(failure) => {
                let error = failure
                    .contract_error()
                    .unwrap_or(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady);
                return Err(RuntimeClosedRecoveryEmptyOpenFailureV2 {
                    state: Box::new(RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2 {
                        owner,
                        gateway,
                        registry,
                        worker: failure.into_state(),
                    }),
                    error: RuntimeClosedRecoveryProductionHandoffErrorV2::Worker(error),
                });
            }
        };
        Ok(RuntimeClosedRecoveryEmptyOpenProcessV2 {
            owner,
            gateway,
            registry,
            worker,
        })
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
            worker,
        } = self;
        drop((gateway, registry, worker));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }
}

struct RuntimeClosedRecoveryOpenProductionObserverV2 {
    observation: std::cell::RefCell<Option<RuntimeOpenProductionObservationInputV2>>,
}

impl RuntimeOpenProductionObservationPortV2 for RuntimeClosedRecoveryOpenProductionObserverV2 {
    type Error = std::convert::Infallible;

    fn observe_open_production(
        &self,
        _request: &RuntimeOpenProductionRequestV2,
    ) -> Result<RuntimeOpenProductionObservationV2, Self::Error> {
        Ok(RuntimeOpenProductionObservationV2::new(
            self.observation
                .borrow_mut()
                .take()
                .expect("open production observation must be consumed exactly once"),
        ))
    }
}

impl RuntimeClosedRecoveryEmptyOpenProcessV2 {
    pub(crate) fn revalidate_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryProductionHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.gateway.current_interrupt_v2().is_some()
            || self
                .gateway
                .observe_exact_current_ready_attestation_v2(
                    self.gateway.coordinator_generation_v2(),
                )
                .is_err()
            || self
                .registry
                .revalidate_production_empty_projection_v2()
                .is_err()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway);
        }
        if self.worker.epoch().process_instance_id()
            != self
                .registry
                .revalidate_production_empty_projection_v2()
                .map_err(|_| RuntimeClosedRecoveryProductionHandoffErrorV2::Registry)?
                .process_instance_id()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Registry);
        }
        Ok(())
    }

    pub(crate) async fn abort_and_shutdown_until_v2(
        self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        let _revalidation = self.revalidate_v2();
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        drop((gateway, registry, worker));
        owner.abort_and_shutdown_until_v2(cleanup_deadline).await
    }

    pub(crate) async fn start_production_owner_v2(
        mut self,
    ) -> Result<
        RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
        RuntimeClosedRecoveryProductionOwnerStartFailureV2,
    > {
        if let Err(error) = self.owner.start_production_renewal_in_place_v2().await {
            return Err(RuntimeClosedRecoveryProductionOwnerStartFailureV2 {
                state: Box::new(self),
                error,
            });
        }
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        let owner = match owner.try_into_production_v2() {
            Ok(owner) => owner,
            Err(owner) => {
                return Err(RuntimeClosedRecoveryProductionOwnerStartFailureV2 {
                    state: Box::new(Self {
                        owner,
                        gateway,
                        registry,
                        worker,
                    }),
                    error: RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation,
                });
            }
        };
        Ok(RuntimeClosedRecoverySupervisedEmptyOpenProcessV2 {
            owner,
            gateway,
            registry,
            worker,
        })
    }
}

impl RuntimeClosedRecoverySupervisedEmptyOpenProcessV2 {
    pub(crate) fn process_generation_v2(&self) -> std::num::NonZeroU64 {
        self.owner.process_generation_v2()
    }

    pub(crate) fn finalizer_generation_v2(
        &self,
    ) -> automation_runtime_worker::RuntimeMutationFinalizerGenerationV1 {
        self.worker.epoch().finalizer_generation()
    }

    pub(crate) fn authorize_ingress_acknowledgement_predecessor_observation_v2(
        &self,
    ) -> RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2 {
        self.worker
            .authorize_ingress_open_acknowledgement_predecessor_observation()
    }

    pub(crate) fn observe_exact_current_ready_attestation_v2(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, crate::RuntimeGatewayReadyObservationErrorV1>
    {
        self.gateway
            .observe_exact_current_ready_attestation_v2(self.worker.coordinator_generation())
    }

    pub(crate) fn arm_gateway_invalidation_trigger_v2(
        &self,
        trigger: crate::process_supervisor::RuntimeProcessInvalidationTriggerV1,
    ) -> bool {
        self.gateway
            .arm_process_invalidation_v2(self.worker.coordinator_generation(), trigger)
    }

    pub(crate) fn bind_gateway_ready_invalidation_observer_v2(
        &self,
        expected_ready: &RuntimeGatewayReadyAttestationV2,
    ) -> RuntimeGatewayReadyInvalidationObserverV2 {
        self.gateway.bind_current_ready_invalidation_observer_v2(
            self.worker.coordinator_generation(),
            expected_ready,
        )
    }

    pub(crate) fn observe_registry_empty_v2(
        &self,
    ) -> Result<
        automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2,
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.registry.revalidate_production_empty_projection_v2()
    }

    pub(crate) async fn observe_current_owner_v2(
        &self,
    ) -> Result<
        crate::RuntimeGatewayOwnerCurrentObservationV1,
        crate::RuntimeGatewayOwnerCurrentObservationErrorV1,
    > {
        self.owner.observe_current_v2().await
    }

    pub(crate) async fn wait_for_owner_successor_v2(
        &self,
        previous_revision: std::num::NonZeroU64,
        deadline: Instant,
    ) -> Result<
        crate::RuntimeGatewayOwnerCurrentObservationV1,
        RuntimeGatewayOwnerStrictSuccessorErrorV2,
    > {
        self.owner
            .wait_for_strict_successor_v2(previous_revision, deadline)
            .await
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

    pub(crate) fn authorize_acknowledgement_refresh_v2(
        self,
        input: RuntimeEmptyOpenAcknowledgementRefreshInputV2,
    ) -> Result<
        RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2,
        RuntimeClosedRecoveryAcknowledgementRefreshFailureV2,
    > {
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        match worker.authorize_ingress_open_acknowledgement_refresh(input) {
            Ok(worker) => Ok(RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2 {
                owner,
                gateway,
                registry,
                worker,
            }),
            Err(failure) => {
                let error = failure.error();
                Err(RuntimeClosedRecoveryAcknowledgementRefreshFailureV2 {
                    state: Box::new(Self {
                        owner,
                        gateway,
                        registry,
                        worker: failure.into_state(),
                    }),
                    error,
                })
            }
        }
    }

    pub(crate) fn revalidate_v2(
        &self,
    ) -> Result<(), RuntimeClosedRecoveryProductionHandoffErrorV2> {
        if self.owner.terminal_status_v2().is_some()
            || self.gateway.current_interrupt_v2().is_some()
            || self.observe_exact_current_ready_attestation_v2().is_err()
        {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Gateway);
        }
        let registry = self
            .registry
            .revalidate_production_empty_projection_v2()
            .map_err(|_| RuntimeClosedRecoveryProductionHandoffErrorV2::Registry)?;
        if self.worker.epoch().process_instance_id() != registry.process_instance_id() {
            return Err(RuntimeClosedRecoveryProductionHandoffErrorV2::Registry);
        }
        Ok(())
    }

    pub(crate) async fn shutdown_until_v2(
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
            worker,
        } = self;
        drop((gateway, registry, worker));
        owner.shutdown_until_v2(cleanup_deadline).await
    }
}

impl RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2 {
    pub(crate) fn into_ingress_acknowledgement_authority_v2(
        self,
    ) -> RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2 {
        RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2::EmptyOpenRefresh(Box::new(self))
    }

    pub(crate) fn operation_mut_v2(
        &mut self,
    ) -> &mut RuntimeIngressOpenAcknowledgementSingleFlightV2 {
        self.worker.operation_mut()
    }

    pub(crate) fn complete_v2(
        self,
        accepted: automation_runtime_worker::RuntimeAcceptedIngressOpenAcknowledgementV2,
    ) -> Result<
        RuntimeClosedRecoverySupervisedEmptyOpenProcessV2,
        RuntimeClosedRecoveryAcknowledgementRefreshCompletionFailureV2,
    > {
        let Self {
            owner,
            gateway,
            registry,
            worker,
        } = self;
        match worker.complete(accepted) {
            Ok(worker) => Ok(RuntimeClosedRecoverySupervisedEmptyOpenProcessV2 {
                owner,
                gateway,
                registry,
                worker,
            }),
            Err(failure) => {
                let error = failure.error();
                Err(
                    RuntimeClosedRecoveryAcknowledgementRefreshCompletionFailureV2 {
                        state: Box::new(Self {
                            owner,
                            gateway,
                            registry,
                            worker: failure.into_refresh(),
                        }),
                        error,
                    },
                )
            }
        }
    }

    pub(crate) async fn shutdown_until_v2(
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
            worker,
        } = self;
        drop((gateway, registry, worker));
        owner.shutdown_until_v2(cleanup_deadline).await
    }
}

impl RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2 {
    pub(crate) fn into_retained_state_v2(
        self,
    ) -> RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2 {
        match self {
            Self::Admission(authority) => {
                let RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2 {
                    lifecycle,
                    operation,
                } = *authority;
                drop(operation);
                RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::Admission(Box::new(
                    lifecycle,
                ))
            }
            Self::EmptyOpenRefresh(refresh) => {
                RuntimeClosedRecoveryIngressAcknowledgementRetainedStateV2::EmptyOpenRefresh(
                    refresh,
                )
            }
        }
    }
}

impl RuntimeIngressAcknowledgementAuthorityV2
    for RuntimeClosedRecoveryIngressAcknowledgementAuthorityV2
{
    type Output = RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2;
    type CompletionError = RuntimeClosedRecoveryIngressAcknowledgementCompletionErrorV2;

    fn operation_mut(&mut self) -> &mut RuntimeIngressOpenAcknowledgementSingleFlightV2 {
        match self {
            Self::Admission(authority) => &mut authority.operation,
            Self::EmptyOpenRefresh(refresh) => refresh.operation_mut_v2(),
        }
    }

    fn complete(
        self,
        accepted: RuntimeAcceptedIngressOpenAcknowledgementV2,
    ) -> Result<Self::Output, (Self, Self::CompletionError)> {
        match self {
            Self::Admission(authority) => {
                let RuntimeClosedRecoveryAdmissionAcknowledgementAuthorityV2 {
                    lifecycle,
                    operation,
                } = *authority;
                drop(operation);
                Ok(
                    RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::Admission {
                        lifecycle: Box::new(lifecycle),
                        accepted: Box::new(accepted),
                    },
                )
            }
            Self::EmptyOpenRefresh(refresh) => {
                let accepted_receipt = accepted.receipt().clone();
                match (*refresh).complete_v2(accepted) {
                    Ok(lifecycle) => Ok(
                        RuntimeClosedRecoveryIngressAcknowledgementOutcomeV2::EmptyOpenRefresh {
                            lifecycle: Box::new(lifecycle),
                            accepted_receipt: Box::new(accepted_receipt),
                        },
                    ),
                    Err(failure) => {
                        let error =
                        RuntimeClosedRecoveryIngressAcknowledgementCompletionErrorV2::EmptyOpenRefresh(
                            failure.error_v2(),
                        );
                        let authority = Self::EmptyOpenRefresh(Box::new(failure.into_state_v2()));
                        Err((authority, error))
                    }
                }
            }
        }
    }
}

pub(crate) struct RuntimeClosedRecoveryAcknowledgementRefreshCompletionFailureV2 {
    state: Box<RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

impl RuntimeClosedRecoveryProductionOwnerStartFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeGatewayOwnerProcessRenewalStartErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoveryEmptyOpenProcessV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryAdmissionAcknowledgementAuthorizationFailureV2 {
    pub(crate) fn error_v2(
        &self,
    ) -> automation_runtime_worker::RuntimeIngressOpenAcknowledgementAuthorizationErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryAcknowledgementRefreshFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoverySupervisedEmptyOpenProcessV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryAcknowledgementRefreshCompletionFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoveryEmptyOpenAcknowledgementRefreshV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryRecoveryResumeFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeClosedRecoveryProductionHandoffErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoveryProductionHandoffProcessV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryEmptyOpenFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeClosedRecoveryProductionHandoffErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoveryAdmissionAcknowledgingProcessV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryProductionHandoffFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeClosedRecoveryProductionHandoffErrorV2 {
        self.error
    }

    pub(crate) fn into_state_v2(self) -> RuntimeClosedRecoveryProcessFrozenProcessV2 {
        *self.state
    }
}

impl RuntimeClosedRecoveryFixedPointAcceptanceFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeClosedRecoveryFixedPointHandoffErrorV2 {
        self.error
    }

    pub(crate) fn into_fixed_point_v2(self) -> RuntimeClosedRecoveryFixedPointV2 {
        *self.fixed_point
    }
}

impl RuntimeClosedRecoveryAdmissionFrozenFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeClosedRecoveryFixedPointHandoffErrorV2 {
        self.error
    }

    pub(crate) async fn abort_and_shutdown_until_v2(
        self,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    > {
        match self.cleanup {
            RuntimeClosedRecoveryAdmissionFrozenCleanupV2::Worker(fixed_point) => {
                fixed_point
                    .abort_and_shutdown_until_v2(cleanup_deadline)
                    .await
            }
            RuntimeClosedRecoveryAdmissionFrozenCleanupV2::Frozen(frozen) => {
                frozen.abort_and_shutdown_until_v2(cleanup_deadline).await
            }
        }
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
    gateway: &mut RuntimeGatewayBootstrapV1,
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
    gateway: &mut RuntimeGatewayBootstrapV1,
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
    gateway: &mut RuntimeGatewayBootstrapV1,
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
        self.gateway
            .successor_for_stale_drop_test_v2()
            .map_err(RuntimeClosedRecoveryBeginErrorV2::Gateway)?;
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
