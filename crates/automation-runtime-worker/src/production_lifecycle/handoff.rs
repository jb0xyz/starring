use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2, RuntimeRecoveryIdV2,
};
use automation_runtime_convergence::ProcessInstanceId;

use super::*;
use crate::{RuntimeClosedRecoveryAuthorityRevisionV2, RuntimeRegistryGlobalObservationSequenceV2};

pub struct RuntimeProductionHandoffRequestV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    recovery_id: RuntimeRecoveryIdV2,
    recovery_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    process_instance_id: ProcessInstanceId,
    connection_epoch: NonZeroU64,
    ready_kind: RuntimeGatewayReadyKindV2,
    paused_admission_revision: NonZeroU64,
    connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pause_sequence: RuntimeGatewayAdmissionSequenceV2,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
}

impl RuntimeProductionHandoffRequestV2 {
    fn from_fixed_point(state: &RuntimeStartupRecoveryFixedPointProcessV2) -> Self {
        let permit = &state.authority.permit;
        let paused = permit.paused_gateway();
        let owner = state.owner_receipt();
        Self {
            coordinator_generation: permit.coordinator_generation(),
            recovery_id: permit.recovery_id().clone(),
            recovery_authority_revision: permit.authority_revision(),
            owner_receipt: owner.clone(),
            process_instance_id: paused.process_instance_id().clone(),
            connection_epoch: paused.connection_epoch(),
            ready_kind: paused.kind(),
            paused_admission_revision: paused.admission_revision(),
            connected_event_sequence: paused.connected_event_sequence(),
            pause_sequence: paused.transition_sequence(),
            registry_observation_sequence: permit
                .registry_evidence()
                .empty_observation()
                .observation_sequence(),
        }
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn recovery_id(&self) -> &RuntimeRecoveryIdV2 {
        &self.recovery_id
    }

    pub fn recovery_authority_revision(&self) -> RuntimeClosedRecoveryAuthorityRevisionV2 {
        self.recovery_authority_revision
    }

    pub fn owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.owner_receipt.lease_id
    }

    pub fn owner_revision(&self) -> NonZeroU64 {
        self.owner_receipt.owner_revision
    }

    pub fn owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.owner_receipt
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.connection_epoch
    }

    pub fn ready_kind(&self) -> RuntimeGatewayReadyKindV2 {
        self.ready_kind
    }

    pub fn paused_admission_revision(&self) -> NonZeroU64 {
        self.paused_admission_revision
    }

    pub fn connected_event_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.connected_event_sequence
    }

    pub fn pause_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.pause_sequence
    }

    pub fn registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }
}

impl Debug for RuntimeProductionHandoffRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionHandoffRequestV2(<redacted>)")
    }
}

pub struct RuntimeProductionHandoffObservationInputV2 {
    pub coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    pub recovery_id: RuntimeRecoveryIdV2,
    pub recovery_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    pub finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    pub startup_intake_sealed: bool,
    pub startup_jobs_settled: bool,
    pub supervisors_started: bool,
}

impl Debug for RuntimeProductionHandoffObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionHandoffObservationInputV2(<redacted>)")
    }
}

pub struct RuntimeProductionHandoffObservationV2 {
    input: RuntimeProductionHandoffObservationInputV2,
}

impl RuntimeProductionHandoffObservationV2 {
    pub fn new(input: RuntimeProductionHandoffObservationInputV2) -> Self {
        Self { input }
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.input.finalizer_generation
    }
}

impl Debug for RuntimeProductionHandoffObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionHandoffObservationV2(<redacted>)")
    }
}

pub trait RuntimeProductionHandoffObservationPortV2 {
    type Error;

    fn observe_production_handoff(
        &self,
        request: &RuntimeProductionHandoffRequestV2,
    ) -> Result<RuntimeProductionHandoffObservationV2, Self::Error>;
}

pub struct RuntimeRecoveryResumePermitV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    recovery_id: RuntimeRecoveryIdV2,
    recovery_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    process_instance_id: ProcessInstanceId,
    connection_epoch: NonZeroU64,
    ready_kind: RuntimeGatewayReadyKindV2,
    paused_admission_revision: NonZeroU64,
    connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pause_sequence: RuntimeGatewayAdmissionSequenceV2,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
}

impl RuntimeRecoveryResumePermitV2 {
    fn from_handoff(
        request: &RuntimeProductionHandoffRequestV2,
        observation: &RuntimeProductionHandoffObservationV2,
    ) -> Self {
        Self {
            coordinator_generation: request.coordinator_generation,
            recovery_id: request.recovery_id.clone(),
            recovery_authority_revision: request.recovery_authority_revision,
            owner_receipt: observation.input.owner_receipt.clone(),
            process_instance_id: request.process_instance_id.clone(),
            connection_epoch: request.connection_epoch,
            ready_kind: request.ready_kind,
            paused_admission_revision: request.paused_admission_revision,
            connected_event_sequence: request.connected_event_sequence,
            pause_sequence: request.pause_sequence,
            registry_observation_sequence: request.registry_observation_sequence,
            finalizer_generation: observation.finalizer_generation(),
        }
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn recovery_id(&self) -> &RuntimeRecoveryIdV2 {
        &self.recovery_id
    }

    pub fn recovery_authority_revision(&self) -> RuntimeClosedRecoveryAuthorityRevisionV2 {
        self.recovery_authority_revision
    }

    pub fn owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.owner_receipt.lease_id
    }

    pub fn owner_revision(&self) -> NonZeroU64 {
        self.owner_receipt.owner_revision
    }

    pub fn owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.owner_receipt
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.connection_epoch
    }

    pub fn ready_kind(&self) -> RuntimeGatewayReadyKindV2 {
        self.ready_kind
    }

    pub fn paused_admission_revision(&self) -> NonZeroU64 {
        self.paused_admission_revision
    }

    pub fn connected_event_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.connected_event_sequence
    }

    pub fn pause_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.pause_sequence
    }

    pub fn registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.finalizer_generation
    }
}

impl Debug for RuntimeRecoveryResumePermitV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryResumePermitV2(<redacted>)")
    }
}

pub struct RuntimeProductionHandoffProcessV2 {
    pub(super) fixed_point: RuntimeStartupRecoveryFixedPointProcessV2,
    observation: RuntimeProductionHandoffObservationV2,
    pub(super) resume_permit: RuntimeRecoveryResumePermitV2,
}

impl RuntimeProductionHandoffProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::ProductionHandoff
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.resume_permit.coordinator_generation()
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        self.resume_permit.process_instance_id()
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.observation.finalizer_generation()
    }

    pub fn recovery_resume_permit(&self) -> &RuntimeRecoveryResumePermitV2 {
        &self.resume_permit
    }

    #[cfg(test)]
    pub(super) fn replace_coordinator_generation_for_test(
        &mut self,
        generation: RuntimeGatewayCoordinatorGenerationV2,
    ) {
        self.resume_permit.coordinator_generation = generation;
    }
}

impl Debug for RuntimeProductionHandoffProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionHandoffProcessV2(<redacted>)")
    }
}

impl RuntimeStartupRecoveryFixedPointProcessV2 {
    pub fn begin_production_handoff<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeProductionHandoffProcessV2,
        RuntimeProductionTransitionFailureV2<Self, P::Error>,
    >
    where
        P: RuntimeProductionHandoffObservationPortV2,
    {
        let request = RuntimeProductionHandoffRequestV2::from_fixed_point(&self);
        let observation = match port.observe_production_handoff(&request) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeProductionTransitionFailureV2::port(self, error));
            }
        };
        if let Err(error) = validate_handoff(&request, &observation) {
            return Err(RuntimeProductionTransitionFailureV2::contract(self, error));
        }
        let resume_permit = RuntimeRecoveryResumePermitV2::from_handoff(&request, &observation);
        Ok(RuntimeProductionHandoffProcessV2 {
            fixed_point: self,
            observation,
            resume_permit,
        })
    }
}

fn validate_handoff(
    request: &RuntimeProductionHandoffRequestV2,
    observation: &RuntimeProductionHandoffObservationV2,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    let observed = &observation.input;
    if observed.coordinator_generation != request.coordinator_generation
        || observed.recovery_id != request.recovery_id
        || observed.recovery_authority_revision != request.recovery_authority_revision
        || observed.process_instance_id != request.process_instance_id
        || observed.connection_epoch != request.connection_epoch
        || observed.paused_admission_revision != request.paused_admission_revision
        || observed.connected_event_sequence != request.connected_event_sequence
        || observed.pause_sequence != request.pause_sequence
        || observed.registry_observation_sequence != request.registry_observation_sequence
    {
        return Err(RuntimeProductionLifecycleErrorV2::HandoffEvidenceMismatch);
    }
    if !same_owner(&request.owner_receipt, &observed.owner_receipt)
        || observed.owner_receipt.database_now < request.owner_receipt.database_now
        || observed.owner_receipt.database_lease_duration().is_none()
    {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    if !observed.startup_intake_sealed {
        return Err(RuntimeProductionLifecycleErrorV2::StartupIntakeNotSealed);
    }
    if !observed.startup_jobs_settled {
        return Err(RuntimeProductionLifecycleErrorV2::StartupJobsUnsettled);
    }
    if !observed.supervisors_started {
        return Err(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady);
    }
    bounded_generation(NonZeroU64::new(observed.finalizer_generation.get()).expect("nonzero"))
}
