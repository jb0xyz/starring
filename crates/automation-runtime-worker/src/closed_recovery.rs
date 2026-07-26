use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{RuntimeGatewayOwnerLeaseReceiptV1, RuntimeRecoveryIdV2};
use chrono::{DateTime, Utc};

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
pub(crate) struct RuntimeClosedRecoveryOperationAuthorityV2 {
    _private: (),
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
    operation_authority: Option<RuntimeClosedRecoveryOperationAuthorityV2>,
    last_startup_observation_database_now: Option<DateTime<Utc>>,
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
            operation_authority: Some(RuntimeClosedRecoveryOperationAuthorityV2 { _private: () }),
            last_startup_observation_database_now: None,
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

    pub(crate) fn operation_authority_is_available(&self) -> bool {
        self.operation_authority.is_some()
    }

    pub(crate) fn take_operation_authority(
        &mut self,
    ) -> Option<RuntimeClosedRecoveryOperationAuthorityV2> {
        self.operation_authority.take()
    }

    pub(crate) fn last_startup_observation_database_now(&self) -> Option<DateTime<Utc>> {
        self.last_startup_observation_database_now
    }

    pub(crate) fn restore_operation_authority(
        &mut self,
        authority: RuntimeClosedRecoveryOperationAuthorityV2,
        database_now: DateTime<Utc>,
    ) -> Option<RuntimeClosedRecoveryAuthorityRevisionV2> {
        if self.operation_authority.is_some() {
            return None;
        }
        let authority_revision = self.authority_revision.successor()?;
        self.operation_authority = Some(authority);
        self.last_startup_observation_database_now = Some(database_now);
        self.authority_revision = authority_revision;
        Some(authority_revision)
    }

    pub(crate) fn refresh_readiness(
        &mut self,
        readiness: RuntimeCapabilityReadinessSetV2,
    ) -> Option<(
        RuntimeClosedRecoveryAuthorityRevisionV2,
        RuntimeClosedRecoveryOperationAuthorityV2,
    )> {
        let authority_revision = self.authority_revision.successor()?;
        let authority = self.take_operation_authority()?;
        self.readiness = readiness;
        self.authority_revision = authority_revision;
        Some((authority_revision, authority))
    }

    pub(crate) fn advance_fixed_point(
        &mut self,
        database_now: DateTime<Utc>,
    ) -> Option<RuntimeClosedRecoveryAuthorityRevisionV2> {
        if self.operation_authority.is_some() {
            return None;
        }
        let authority_revision = self.authority_revision.successor()?;
        self.last_startup_observation_database_now = Some(database_now);
        self.authority_revision = authority_revision;
        Some(authority_revision)
    }

    #[cfg(test)]
    pub(crate) fn exhaust_authority_revision_for_test(&mut self) {
        self.authority_revision =
            RuntimeClosedRecoveryAuthorityRevisionV2(NonZeroU64::new(i64::MAX as u64).unwrap());
    }

    #[cfg(test)]
    pub(crate) fn prepare_authority_revision_overflow_for_test(&mut self) {
        self.authority_revision =
            RuntimeClosedRecoveryAuthorityRevisionV2(NonZeroU64::new(i64::MAX as u64 - 1).unwrap());
    }
}

impl Debug for RuntimeClosedDrainRecoveryPermitV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClosedDrainRecoveryPermitV2(<redacted>)")
    }
}
