use std::num::NonZeroU64;

use crate::{
    accept_validated_startup_recovery_observation_v2, authorize_startup_recovery_iteration_v2,
    authorize_startup_recovery_observation_v2, startup_recovery_fixed_point_matches_permit_v2,
    validate_startup_recovery_observation_v2, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeAuthorizedStartupRecoveryIterationV2, RuntimeAuthorizedStartupRecoveryObservationV2,
    RuntimeClosedDrainRecoveryPermitV2, RuntimeClosedRecoveryInputV2,
    RuntimeCompletedStartupRecoveryObservationV2, RuntimeStartupRecoveryFixedPointProofV2,
    RuntimeStartupRecoveryObservationAcceptanceErrorV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeGatewayCoordinatorGenerationV2(NonZeroU64);

impl RuntimeGatewayCoordinatorGenerationV2 {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }

    fn successor(self) -> Option<Self> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeGatewayEmergencyCauseV2 {
    Starting,
    TransportDisconnected,
    ControlOrphaned,
    OwnershipUncertain,
    CapabilityNotReady,
    ProtocolViolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeGatewayInvalidationCauseV2 {
    TransportDisconnected,
    ControlOrphaned,
    OwnershipUncertain,
    CapabilityNotReady,
    ProtocolViolation,
}

impl From<RuntimeGatewayInvalidationCauseV2> for RuntimeGatewayEmergencyCauseV2 {
    fn from(value: RuntimeGatewayInvalidationCauseV2) -> Self {
        match value {
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected => Self::TransportDisconnected,
            RuntimeGatewayInvalidationCauseV2::ControlOrphaned => Self::ControlOrphaned,
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain => Self::OwnershipUncertain,
            RuntimeGatewayInvalidationCauseV2::CapabilityNotReady => Self::CapabilityNotReady,
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation => Self::ProtocolViolation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayClosedSnapshotV2 {
    Emergency {
        generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayEmergencyCauseV2,
    },
    RecoveryPending {
        generation: RuntimeGatewayCoordinatorGenerationV2,
        recovery_id: automation_runtime_controller::RuntimeRecoveryIdV2,
        authority_revision: crate::RuntimeClosedRecoveryAuthorityRevisionV2,
    },
    Shutdown {
        generation: RuntimeGatewayCoordinatorGenerationV2,
    },
}

impl RuntimeGatewayClosedSnapshotV2 {
    pub fn generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        match self {
            Self::Emergency { generation, .. }
            | Self::RecoveryPending { generation, .. }
            | Self::Shutdown { generation } => *generation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayClosedTransitionErrorV2 {
    #[error("runtime gateway coordinator generation is stale")]
    StaleGeneration,
    #[error("runtime gateway coordinator generation overflowed")]
    GenerationOverflow,
    #[error("runtime gateway coordinator is shut down")]
    Shutdown,
    #[error("runtime gateway coordinator is not in emergency")]
    NotEmergency,
    #[error("runtime paused gateway coordinator generation does not match")]
    PausedGatewayGenerationMismatch,
    #[error("runtime closed recovery process identity does not match")]
    ProcessInstanceMismatch,
    #[error("runtime gateway owner receipt is not current")]
    OwnerReceiptNotCurrent,
    #[error("runtime closed recovery sequence is outside the persistence domain")]
    EvidenceSequenceOutOfRange,
    #[error("runtime closed recovery permit is stale")]
    StaleRecoveryPermit,
    #[error("runtime capability readiness authority does not match")]
    CapabilityReadinessAuthorityMismatch,
    #[error("runtime capability readiness is not an exact freshness successor")]
    CapabilityReadinessNotSuccessor,
    #[error("runtime closed recovery authority revision overflowed")]
    AuthorityRevisionOverflow,
    #[error("runtime closed recovery operation is already in flight")]
    RecoveryOperationInFlight,
    #[error("runtime closed recovery operation is not in flight")]
    RecoveryOperationNotInFlight,
    #[error("runtime closed recovery iteration authority is stale")]
    StaleRecoveryIterationAuthority,
    #[error("runtime startup recovery fixed point authority is stale")]
    StaleRecoveryFixedPointAuthority,
    #[error("runtime startup recovery observation was rejected")]
    StartupRecoveryObservation(RuntimeStartupRecoveryObservationAcceptanceErrorV2),
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeGatewayClosedLifecycleV2 {
    snapshot: RuntimeGatewayClosedSnapshotV2,
}

impl RuntimeGatewayClosedLifecycleV2 {
    pub fn starting() -> Self {
        Self {
            snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: RuntimeGatewayCoordinatorGenerationV2::FIRST,
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
            },
        }
    }

    pub fn snapshot(&self) -> RuntimeGatewayClosedSnapshotV2 {
        self.snapshot.clone()
    }

    pub fn begin_recovery(
        &mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        input: RuntimeClosedRecoveryInputV2,
    ) -> Result<
        (
            RuntimeGatewayClosedSnapshotV2,
            RuntimeClosedDrainRecoveryPermitV2,
        ),
        RuntimeGatewayClosedTransitionErrorV2,
    > {
        self.require_generation(expected_generation)?;
        match &self.snapshot {
            RuntimeGatewayClosedSnapshotV2::Emergency { .. } => {}
            RuntimeGatewayClosedSnapshotV2::RecoveryPending { .. } => {
                return Err(RuntimeGatewayClosedTransitionErrorV2::NotEmergency);
            }
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. } => {
                return Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown);
            }
        }
        let Some(successor_generation) = expected_generation.successor() else {
            self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown {
                generation: expected_generation,
            };
            return Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow);
        };
        let (recovery_id, owner_receipt, readiness, paused_gateway, registry_evidence) =
            input.into_parts();
        if paused_gateway.coordinator_generation() != expected_generation {
            return Err(RuntimeGatewayClosedTransitionErrorV2::PausedGatewayGenerationMismatch);
        }
        let process_instance_id = &owner_receipt.lease_id.process_instance_id;
        if paused_gateway.process_instance_id() != process_instance_id
            || registry_evidence.process_instance_id() != process_instance_id
        {
            return Err(RuntimeGatewayClosedTransitionErrorV2::ProcessInstanceMismatch);
        }
        if owner_receipt.database_lease_duration().is_none() {
            return Err(RuntimeGatewayClosedTransitionErrorV2::OwnerReceiptNotCurrent);
        }
        let persistence_max = i64::MAX as u64;
        if successor_generation.get() > persistence_max
            || owner_receipt.lease_id.lease_epoch.get() > persistence_max
            || owner_receipt.owner_revision.get() > persistence_max
            || paused_gateway.connection_epoch().get() > persistence_max
            || paused_gateway.admission_revision().get() > persistence_max
            || paused_gateway.transition_sequence().get() > persistence_max
            || registry_evidence
                .empty_observation()
                .observation_sequence()
                .get()
                > persistence_max
        {
            return Err(RuntimeGatewayClosedTransitionErrorV2::EvidenceSequenceOutOfRange);
        }
        let generation = successor_generation;
        let authority_revision = crate::RuntimeClosedRecoveryAuthorityRevisionV2::FIRST;
        self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id: recovery_id.clone(),
            authority_revision,
        };
        let permit = RuntimeClosedDrainRecoveryPermitV2::new(
            expected_generation,
            generation,
            recovery_id,
            owner_receipt,
            readiness,
            paused_gateway,
            registry_evidence,
        );
        Ok((self.snapshot.clone(), permit))
    }

    pub fn validate_recovery_permit(
        &self,
        permit: &RuntimeClosedDrainRecoveryPermitV2,
    ) -> Result<(), RuntimeGatewayClosedTransitionErrorV2> {
        match &self.snapshot {
            RuntimeGatewayClosedSnapshotV2::RecoveryPending {
                generation,
                recovery_id,
                authority_revision,
            } if *generation == permit.coordinator_generation()
                && recovery_id == permit.recovery_id()
                && *authority_revision == permit.authority_revision() =>
            {
                Ok(())
            }
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. } => {
                Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
            }
            RuntimeGatewayClosedSnapshotV2::Emergency { .. }
            | RuntimeGatewayClosedSnapshotV2::RecoveryPending { .. } => {
                Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
            }
        }
    }

    pub fn refresh_recovery_readiness(
        &mut self,
        permit: &mut RuntimeClosedDrainRecoveryPermitV2,
        readiness: crate::RuntimeCapabilityReadinessSetV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryIterationV2, RuntimeGatewayClosedTransitionErrorV2>
    {
        self.validate_recovery_permit(permit)?;
        if !permit.operation_authority_is_available() {
            let generation = permit.coordinator_generation();
            self.invalidate(
                generation,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            )?;
            return Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight);
        }
        if !permit.readiness().has_same_authority_as(&readiness) {
            let generation = permit.coordinator_generation();
            self.invalidate(
                generation,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            )?;
            return Err(
                RuntimeGatewayClosedTransitionErrorV2::CapabilityReadinessAuthorityMismatch,
            );
        }
        if !readiness.has_strictly_newer_checks_than(permit.readiness()) {
            let generation = permit.coordinator_generation();
            self.invalidate(
                generation,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            )?;
            return Err(RuntimeGatewayClosedTransitionErrorV2::CapabilityReadinessNotSuccessor);
        }
        let generation = permit.coordinator_generation();
        let recovery_id = permit.recovery_id().clone();
        let Some((authority_revision, operation_authority)) = permit.refresh_readiness(readiness)
        else {
            self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown { generation };
            return Err(RuntimeGatewayClosedTransitionErrorV2::AuthorityRevisionOverflow);
        };
        let iteration = authorize_startup_recovery_iteration_v2(permit, operation_authority);
        self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            authority_revision,
        };
        Ok(iteration)
    }

    pub fn begin_startup_recovery_observation(
        &mut self,
        permit: &mut RuntimeClosedDrainRecoveryPermitV2,
        iteration: RuntimeAuthorizedStartupRecoveryIterationV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryObservationV2, RuntimeGatewayClosedTransitionErrorV2>
    {
        self.validate_recovery_permit(permit)?;
        if permit.operation_authority_is_available() {
            let generation = permit.coordinator_generation();
            self.invalidate(
                generation,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            )?;
            return Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationNotInFlight);
        }
        let Some(authorization) = authorize_startup_recovery_observation_v2(permit, iteration)
        else {
            let generation = permit.coordinator_generation();
            self.invalidate(
                generation,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            )?;
            return Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryIterationAuthority);
        };
        Ok(authorization)
    }

    pub fn complete_startup_recovery_observation(
        &mut self,
        permit: &mut RuntimeClosedDrainRecoveryPermitV2,
        completed: RuntimeCompletedStartupRecoveryObservationV2,
    ) -> Result<RuntimeAcceptedStartupRecoveryOutcomeV2, RuntimeGatewayClosedTransitionErrorV2>
    {
        self.validate_recovery_permit(permit)?;
        if permit.operation_authority_is_available() {
            let generation = permit.coordinator_generation();
            self.invalidate(
                generation,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            )?;
            return Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationNotInFlight);
        }
        let validated = match validate_startup_recovery_observation_v2(permit, completed) {
            Ok(validated) => validated,
            Err(error) => {
                let generation = permit.coordinator_generation();
                self.invalidate(generation, startup_observation_invalidation_cause(error))?;
                return Err(
                    RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryObservation(error),
                );
            }
        };
        let generation = permit.coordinator_generation();
        let recovery_id = permit.recovery_id().clone();
        let Some((authority_revision, outcome)) =
            accept_validated_startup_recovery_observation_v2(permit, validated)
        else {
            self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown { generation };
            return Err(RuntimeGatewayClosedTransitionErrorV2::AuthorityRevisionOverflow);
        };
        self.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            authority_revision,
        };
        Ok(outcome)
    }

    pub fn validate_startup_recovery_fixed_point(
        &self,
        permit: &RuntimeClosedDrainRecoveryPermitV2,
        proof: &RuntimeStartupRecoveryFixedPointProofV2,
    ) -> Result<(), RuntimeGatewayClosedTransitionErrorV2> {
        self.validate_recovery_permit(permit)?;
        if startup_recovery_fixed_point_matches_permit_v2(permit, proof) {
            Ok(())
        } else {
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryFixedPointAuthority)
        }
    }

    pub fn invalidate(
        &mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<RuntimeGatewayClosedSnapshotV2, RuntimeGatewayClosedTransitionErrorV2> {
        self.require_generation(expected_generation)?;
        if matches!(
            &self.snapshot,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ) {
            return Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown);
        }
        let generation = self.advance_generation()?;
        self.snapshot = RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: cause.into(),
        };
        Ok(self.snapshot.clone())
    }

    pub fn shutdown(
        &mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<RuntimeGatewayClosedSnapshotV2, RuntimeGatewayClosedTransitionErrorV2> {
        self.require_generation(expected_generation)?;
        if matches!(
            &self.snapshot,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ) {
            return Ok(self.snapshot.clone());
        }
        let generation = self.advance_generation()?;
        self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown { generation };
        Ok(self.snapshot.clone())
    }

    fn require_generation(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<(), RuntimeGatewayClosedTransitionErrorV2> {
        if self.snapshot.generation() != expected_generation {
            return Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration);
        }
        Ok(())
    }

    fn advance_generation(
        &mut self,
    ) -> Result<RuntimeGatewayCoordinatorGenerationV2, RuntimeGatewayClosedTransitionErrorV2> {
        let current = self.snapshot.generation();
        let Some(successor) = current.successor() else {
            self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown {
                generation: current,
            };
            return Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow);
        };
        Ok(successor)
    }
}

fn startup_observation_invalidation_cause(
    error: RuntimeStartupRecoveryObservationAcceptanceErrorV2,
) -> RuntimeGatewayInvalidationCauseV2 {
    match error {
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch
        | RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerNotCurrent => {
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain
        }
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::Ambiguous
        | RuntimeStartupRecoveryObservationAcceptanceErrorV2::InvalidObservation => {
            RuntimeGatewayInvalidationCauseV2::CapabilityNotReady
        }
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::CorrelationMismatch
        | RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseClockRegressed
        | RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseTimeMismatch => {
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation
        }
    }
}

#[cfg(test)]
#[path = "gateway_lifecycle_tests.rs"]
mod tests;
