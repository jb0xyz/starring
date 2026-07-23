use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{RuntimeGatewayOwnerLeaseReceiptV1, RuntimeRecoveryIdV2};

use crate::{
    RuntimeCapabilityReadinessSetV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimePausedGatewayObservationV2, RuntimeRegistryRecoveryEmptyObservationV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeClosedRecoveryAuthorityRevisionV2(NonZeroU64);

impl RuntimeClosedRecoveryAuthorityRevisionV2 {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    fn successor(self) -> Option<Self> {
        self.get()
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .and_then(NonZeroU64::new)
            .map(Self)
    }
}

#[derive(PartialEq, Eq)]
pub enum RuntimeClosedRecoveryRegistryEvidenceV2 {
    Empty(RuntimeRegistryRecoveryEmptyObservationV2),
}

impl RuntimeClosedRecoveryRegistryEvidenceV2 {
    pub fn process_instance_id(&self) -> &automation_runtime_convergence::ProcessInstanceId {
        match self {
            Self::Empty(observation) => observation.process_instance_id(),
        }
    }

    pub fn empty_observation(&self) -> &RuntimeRegistryRecoveryEmptyObservationV2 {
        match self {
            Self::Empty(observation) => observation,
        }
    }
}

impl Debug for RuntimeClosedRecoveryRegistryEvidenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryRegistryEvidenceV2(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimeClosedRecoveryInputV2 {
    recovery_id: RuntimeRecoveryIdV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    paused_gateway: RuntimePausedGatewayObservationV2,
    registry_evidence: RuntimeClosedRecoveryRegistryEvidenceV2,
}

impl RuntimeClosedRecoveryInputV2 {
    pub fn new(
        recovery_id: RuntimeRecoveryIdV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        readiness: RuntimeCapabilityReadinessSetV2,
        paused_gateway: RuntimePausedGatewayObservationV2,
        registry_evidence: RuntimeClosedRecoveryRegistryEvidenceV2,
    ) -> Self {
        Self {
            recovery_id,
            owner_receipt,
            readiness,
            paused_gateway,
            registry_evidence,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeRecoveryIdV2,
        RuntimeGatewayOwnerLeaseReceiptV1,
        RuntimeCapabilityReadinessSetV2,
        RuntimePausedGatewayObservationV2,
        RuntimeClosedRecoveryRegistryEvidenceV2,
    ) {
        (
            self.recovery_id,
            self.owner_receipt,
            self.readiness,
            self.paused_gateway,
            self.registry_evidence,
        )
    }
}

impl Debug for RuntimeClosedRecoveryInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedRecoveryInputV2(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimeClosedDrainRecoveryPermitV2 {
    originating_emergency_generation: RuntimeGatewayCoordinatorGenerationV2,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    recovery_id: RuntimeRecoveryIdV2,
    authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    paused_gateway: RuntimePausedGatewayObservationV2,
    registry_evidence: RuntimeClosedRecoveryRegistryEvidenceV2,
}

impl RuntimeClosedDrainRecoveryPermitV2 {
    pub(crate) fn new(
        originating_emergency_generation: RuntimeGatewayCoordinatorGenerationV2,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        recovery_id: RuntimeRecoveryIdV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        readiness: RuntimeCapabilityReadinessSetV2,
        paused_gateway: RuntimePausedGatewayObservationV2,
        registry_evidence: RuntimeClosedRecoveryRegistryEvidenceV2,
    ) -> Self {
        Self {
            originating_emergency_generation,
            coordinator_generation,
            recovery_id,
            authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2::FIRST,
            owner_receipt,
            readiness,
            paused_gateway,
            registry_evidence,
        }
    }

    pub fn originating_emergency_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.originating_emergency_generation
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn recovery_id(&self) -> &RuntimeRecoveryIdV2 {
        &self.recovery_id
    }

    pub fn authority_revision(&self) -> RuntimeClosedRecoveryAuthorityRevisionV2 {
        self.authority_revision
    }

    pub fn owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.owner_receipt
    }

    pub fn readiness(&self) -> &RuntimeCapabilityReadinessSetV2 {
        &self.readiness
    }

    pub fn paused_gateway(&self) -> &RuntimePausedGatewayObservationV2 {
        &self.paused_gateway
    }

    pub fn registry_evidence(&self) -> &RuntimeClosedRecoveryRegistryEvidenceV2 {
        &self.registry_evidence
    }

    pub(crate) fn refresh_readiness(
        &mut self,
        readiness: RuntimeCapabilityReadinessSetV2,
    ) -> Option<RuntimeClosedRecoveryAuthorityRevisionV2> {
        let authority_revision = self.authority_revision.successor()?;
        self.readiness = readiness;
        self.authority_revision = authority_revision;
        Some(authority_revision)
    }

    #[cfg(test)]
    pub(crate) fn exhaust_authority_revision_for_test(&mut self) {
        self.authority_revision =
            RuntimeClosedRecoveryAuthorityRevisionV2(NonZeroU64::new(i64::MAX as u64).unwrap());
    }
}

impl Debug for RuntimeClosedDrainRecoveryPermitV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedDrainRecoveryPermitV2(<redacted>)")
    }
}
