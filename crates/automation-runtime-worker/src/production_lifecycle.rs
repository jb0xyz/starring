use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeWriterFenceGenerationV1,
};
use automation_runtime_convergence::ProcessInstanceId;

use crate::{
    RuntimeClosedDrainRecoveryPermitV2, RuntimeGatewayClosedLifecycleV2,
    RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeStartupRecoveryFixedPointProofV2,
};

mod admission;
mod handoff;
mod shutdown;

pub use admission::{
    RuntimeAdmissionAcknowledgingProcessV2, RuntimeEmptyOpenEpochV2, RuntimeEmptyOpenProcessV2,
    RuntimeIngressOpenAcknowledgementObservationInputV2,
    RuntimeIngressOpenAcknowledgementObservationV2, RuntimeOpenProductionObservationInputV2,
    RuntimeOpenProductionObservationPortV2, RuntimeOpenProductionObservationV2,
    RuntimeOpenProductionRequestV2, RuntimeRecoveryResumeObservationInputV2,
    RuntimeRecoveryResumeObservationV2, RuntimeRecoveryResumePortV2,
};
pub use handoff::{
    RuntimeProductionHandoffObservationInputV2, RuntimeProductionHandoffObservationPortV2,
    RuntimeProductionHandoffObservationV2, RuntimeProductionHandoffProcessV2,
    RuntimeProductionHandoffRequestV2, RuntimeRecoveryResumePermitV2,
};
pub use shutdown::{
    RuntimeProductionEmergencyProcessV2, RuntimeProductionInvalidationOutcomeV2,
    RuntimeShutdownCauseV2, RuntimeShuttingDownProcessV2,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeMutationFinalizerGenerationV1(NonZeroU64);

impl RuntimeMutationFinalizerGenerationV1 {
    pub fn new(value: NonZeroU64) -> Result<Self, RuntimeProductionLifecycleErrorV2> {
        bounded_generation(value)?;
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeMaintenanceGateGenerationV2(NonZeroU64);

impl RuntimeMaintenanceGateGenerationV2 {
    pub fn new(value: NonZeroU64) -> Result<Self, RuntimeProductionLifecycleErrorV2> {
        bounded_generation(value)?;
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductionLifecycleStageV2 {
    FixedPoint,
    ProductionHandoff,
    AdmissionAcknowledging,
    OpenProduction,
    Emergency,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductionLifecycleErrorV2 {
    #[error("runtime production fixed point is not current")]
    FixedPoint(RuntimeGatewayClosedTransitionErrorV2),
    #[error("runtime production handoff evidence does not match")]
    HandoffEvidenceMismatch,
    #[error("runtime production startup mutation intake is not sealed")]
    StartupIntakeNotSealed,
    #[error("runtime production startup mutation jobs are not settled")]
    StartupJobsUnsettled,
    #[error("runtime production supervisors are not ready")]
    SupervisorsNotReady,
    #[error("runtime production resume permit does not match")]
    ResumePermitMismatch,
    #[error("runtime production connection epoch is stale")]
    StaleConnectionEpoch,
    #[error("runtime production admission revision is stale")]
    StaleAdmissionRevision,
    #[error("runtime production gateway ready evidence does not match")]
    GatewayReadyMismatch,
    #[error("runtime production gateway was not explicitly resumed")]
    ExplicitResumeMissing,
    #[error("runtime production writer fence does not match")]
    WriterFenceMismatch,
    #[error("runtime production maintenance gate does not match")]
    MaintenanceGateMismatch,
    #[error("runtime production registry evidence does not match")]
    RegistryMismatch,
    #[error("runtime production gateway owner does not match")]
    OwnerMismatch,
    #[error("runtime production capability readiness does not match")]
    ReadinessMismatch,
    #[error("runtime production finalizer generation does not match")]
    FinalizerGenerationMismatch,
    #[error("runtime production ingress acknowledgement does not match")]
    IngressAcknowledgementMismatch,
    #[error("runtime production ingress acknowledgement is not current")]
    IngressAcknowledgementNotCurrent,
    #[error("runtime production coordinator generation is stale")]
    StaleGeneration,
    #[error("runtime production sequence is outside the persistence domain")]
    SequenceOutOfRange,
    #[error("runtime production generation overflowed")]
    GenerationOverflow,
}

enum RuntimeProductionTransitionFailureKindV2<E> {
    Port(E),
    Contract(RuntimeProductionLifecycleErrorV2),
}

pub struct RuntimeProductionTransitionFailureV2<S, E> {
    state: Box<S>,
    kind: RuntimeProductionTransitionFailureKindV2<E>,
}

impl<S, E> RuntimeProductionTransitionFailureV2<S, E> {
    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn port_error(&self) -> Option<&E> {
        match &self.kind {
            RuntimeProductionTransitionFailureKindV2::Port(error) => Some(error),
            RuntimeProductionTransitionFailureKindV2::Contract(_) => None,
        }
    }

    pub fn contract_error(&self) -> Option<RuntimeProductionLifecycleErrorV2> {
        match &self.kind {
            RuntimeProductionTransitionFailureKindV2::Contract(error) => Some(*error),
            RuntimeProductionTransitionFailureKindV2::Port(_) => None,
        }
    }

    pub fn into_state(self) -> S {
        *self.state
    }

    pub(super) fn port(state: S, error: E) -> Self {
        Self {
            state: Box::new(state),
            kind: RuntimeProductionTransitionFailureKindV2::Port(error),
        }
    }

    pub(super) fn contract(state: S, error: RuntimeProductionLifecycleErrorV2) -> Self {
        Self {
            state: Box::new(state),
            kind: RuntimeProductionTransitionFailureKindV2::Contract(error),
        }
    }
}

impl<S, E> Debug for RuntimeProductionTransitionFailureV2<S, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionTransitionFailureV2(<redacted>)")
    }
}

struct RuntimeProductionFixedPointAuthorityV2 {
    lifecycle: RuntimeGatewayClosedLifecycleV2,
    permit: RuntimeClosedDrainRecoveryPermitV2,
    proof: RuntimeStartupRecoveryFixedPointProofV2,
}

pub struct RuntimeStartupRecoveryFixedPointProcessV2 {
    authority: RuntimeProductionFixedPointAuthorityV2,
}

impl RuntimeStartupRecoveryFixedPointProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::FixedPoint
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.authority.permit.coordinator_generation()
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self
            .authority
            .permit
            .owner_receipt()
            .lease_id
            .process_instance_id
    }

    pub fn acknowledged_product_handoff_count(&self) -> u32 {
        self.authority.proof.acknowledged_product_handoff_count()
    }

    pub(super) fn owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        self.authority.permit.owner_receipt()
    }

    pub(super) fn writer_fence_is_bounded(
        generation: RuntimeWriterFenceGenerationV1,
    ) -> Result<(), RuntimeProductionLifecycleErrorV2> {
        bounded_generation(generation.into_non_zero())
    }
}

impl Debug for RuntimeStartupRecoveryFixedPointProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStartupRecoveryFixedPointProcessV2(<redacted>)")
    }
}

pub struct RuntimeProductionFixedPointAcceptanceFailureV2 {
    authority: Box<RuntimeProductionFixedPointAcceptanceAuthorityV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

struct RuntimeProductionFixedPointAcceptanceAuthorityV2 {
    lifecycle: RuntimeGatewayClosedLifecycleV2,
    permit: RuntimeClosedDrainRecoveryPermitV2,
    proof: RuntimeStartupRecoveryFixedPointProofV2,
}

impl RuntimeProductionFixedPointAcceptanceFailureV2 {
    pub fn error(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub fn into_parts(
        self,
    ) -> (
        RuntimeGatewayClosedLifecycleV2,
        RuntimeClosedDrainRecoveryPermitV2,
        RuntimeStartupRecoveryFixedPointProofV2,
    ) {
        let RuntimeProductionFixedPointAcceptanceAuthorityV2 {
            lifecycle,
            permit,
            proof,
        } = *self.authority;
        (lifecycle, permit, proof)
    }
}

impl Debug for RuntimeProductionFixedPointAcceptanceFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionFixedPointAcceptanceFailureV2(<redacted>)")
    }
}

impl RuntimeGatewayClosedLifecycleV2 {
    pub fn into_production_fixed_point(
        self,
        permit: RuntimeClosedDrainRecoveryPermitV2,
        proof: RuntimeStartupRecoveryFixedPointProofV2,
    ) -> Result<
        RuntimeStartupRecoveryFixedPointProcessV2,
        RuntimeProductionFixedPointAcceptanceFailureV2,
    > {
        if let Err(error) = self.validate_startup_recovery_fixed_point(&permit, &proof) {
            return Err(RuntimeProductionFixedPointAcceptanceFailureV2 {
                authority: Box::new(RuntimeProductionFixedPointAcceptanceAuthorityV2 {
                    lifecycle: self,
                    permit,
                    proof,
                }),
                error: RuntimeProductionLifecycleErrorV2::FixedPoint(error),
            });
        }
        Ok(RuntimeStartupRecoveryFixedPointProcessV2 {
            authority: RuntimeProductionFixedPointAuthorityV2 {
                lifecycle: self,
                permit,
                proof,
            },
        })
    }
}

pub(super) fn bounded_generation(
    value: NonZeroU64,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    if value.get() > i64::MAX as u64 {
        return Err(RuntimeProductionLifecycleErrorV2::SequenceOutOfRange);
    }
    Ok(())
}

pub(super) fn successor_generation(
    value: RuntimeGatewayCoordinatorGenerationV2,
) -> Result<RuntimeGatewayCoordinatorGenerationV2, RuntimeProductionLifecycleErrorV2> {
    value
        .get()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(NonZeroU64::new)
        .map(RuntimeGatewayCoordinatorGenerationV2::new)
        .ok_or(RuntimeProductionLifecycleErrorV2::GenerationOverflow)
}

pub(super) fn same_owner(
    left: &RuntimeGatewayOwnerLeaseReceiptV1,
    right: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> bool {
    left.lease_id == right.lease_id
        && left.owner_revision == right.owner_revision
        && left.expires_at == right.expires_at
}
