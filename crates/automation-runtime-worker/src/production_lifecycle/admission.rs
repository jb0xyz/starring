use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyAttestationV2, RuntimeRecoveryIdV2,
    RuntimeWriterFenceGenerationV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use super::*;
use crate::{
    RuntimeCapabilityReadinessSetV2, RuntimeClosedRecoveryAuthorityRevisionV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryEmptyObservationV2,
};

pub struct RuntimeRecoveryResumeObservationInputV2 {
    pub coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    pub recovery_id: RuntimeRecoveryIdV2,
    pub recovery_authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub readiness: RuntimeCapabilityReadinessSetV2,
    pub registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    pub finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    pub writer_fence_generation: RuntimeWriterFenceGenerationV1,
    pub writer_fence_open: bool,
    pub maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    pub maintenance_gate_closed: bool,
    pub gateway_ready: RuntimeGatewayReadyAttestationV2,
}

impl Debug for RuntimeRecoveryResumeObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryResumeObservationInputV2(<redacted>)")
    }
}

pub struct RuntimeRecoveryResumeObservationV2 {
    input: RuntimeRecoveryResumeObservationInputV2,
}

impl RuntimeRecoveryResumeObservationV2 {
    pub fn new(input: RuntimeRecoveryResumeObservationInputV2) -> Self {
        Self { input }
    }
}

impl Debug for RuntimeRecoveryResumeObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryResumeObservationV2(<redacted>)")
    }
}

pub trait RuntimeRecoveryResumePortV2 {
    type Error;

    fn resume_or_observe_recovery(
        &self,
        permit: &RuntimeRecoveryResumePermitV2,
    ) -> Result<RuntimeRecoveryResumeObservationV2, Self::Error>;
}

pub struct RuntimeAdmissionAcknowledgingProcessV2 {
    pub(super) _handoff: RuntimeProductionHandoffProcessV2,
    resume: RuntimeRecoveryResumeObservationV2,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
}

impl RuntimeAdmissionAcknowledgingProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::AdmissionAcknowledging
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.resume.input.process_instance_id
    }

    pub fn gateway_ready(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.resume.input.gateway_ready
    }

    pub fn writer_fence_generation(&self) -> RuntimeWriterFenceGenerationV1 {
        self.resume.input.writer_fence_generation
    }

    pub fn closed_maintenance_gate_generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.resume.input.maintenance_gate_generation
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.resume.input.finalizer_generation
    }
}

impl Debug for RuntimeAdmissionAcknowledgingProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAdmissionAcknowledgingProcessV2(<redacted>)")
    }
}

impl RuntimeProductionHandoffProcessV2 {
    pub fn resume_recovery<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeAdmissionAcknowledgingProcessV2,
        RuntimeProductionTransitionFailureV2<Self, P::Error>,
    >
    where
        P: RuntimeRecoveryResumePortV2,
    {
        let observation = match port.resume_or_observe_recovery(&self.resume_permit) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeProductionTransitionFailureV2::port(self, error));
            }
        };
        if let Err(error) = validate_resume(&self, &observation) {
            return Err(RuntimeProductionTransitionFailureV2::contract(self, error));
        }
        let coordinator_generation = match successor_generation(self.coordinator_generation()) {
            Ok(generation) => generation,
            Err(error) => {
                return Err(RuntimeProductionTransitionFailureV2::contract(self, error));
            }
        };
        Ok(RuntimeAdmissionAcknowledgingProcessV2 {
            _handoff: self,
            resume: observation,
            coordinator_generation,
        })
    }
}

fn validate_resume(
    state: &RuntimeProductionHandoffProcessV2,
    observation: &RuntimeRecoveryResumeObservationV2,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    let permit = &state.resume_permit;
    let observed = &observation.input;
    if observed.coordinator_generation != permit.coordinator_generation()
        || observed.recovery_id != *permit.recovery_id()
        || observed.recovery_authority_revision != permit.recovery_authority_revision()
        || observed.process_instance_id != *permit.process_instance_id()
        || observed.connection_epoch != permit.connection_epoch()
        || observed.connected_event_sequence != permit.connected_event_sequence()
        || observed.pause_sequence != permit.pause_sequence()
    {
        return Err(RuntimeProductionLifecycleErrorV2::ResumePermitMismatch);
    }
    if observed.paused_admission_revision != permit.paused_admission_revision() {
        return Err(RuntimeProductionLifecycleErrorV2::StaleAdmissionRevision);
    }
    if observed.gateway_ready.process_instance_id != *permit.process_instance_id()
        || observed.gateway_ready.connection_epoch != permit.connection_epoch()
    {
        return Err(RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch);
    }
    if observed.gateway_ready.kind != permit.ready_kind()
        || observed.gateway_ready.admission_revision != permit.paused_admission_revision()
        || observed.gateway_ready.connected_event_sequence != permit.connected_event_sequence()
    {
        return Err(RuntimeProductionLifecycleErrorV2::GatewayReadyMismatch);
    }
    if !observed.gateway_ready.was_explicitly_resumed()
        || observed.gateway_ready.resume_sequence <= permit.pause_sequence()
    {
        return Err(RuntimeProductionLifecycleErrorV2::ExplicitResumeMissing);
    }
    if !same_owner(state.fixed_point.owner_receipt(), &observed.owner_receipt)
        || observed.owner_receipt.database_now < state.fixed_point.owner_receipt().database_now
        || observed.owner_receipt.database_lease_duration().is_none()
    {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    let fixed_point_readiness = state.fixed_point.authority.permit.readiness();
    if observed.readiness != *fixed_point_readiness
        && (!fixed_point_readiness.has_same_authority_as(&observed.readiness)
            || !observed
                .readiness
                .has_strictly_newer_checks_than(fixed_point_readiness))
    {
        return Err(RuntimeProductionLifecycleErrorV2::ReadinessMismatch);
    }
    if observed.registry_observation_sequence != permit.registry_observation_sequence() {
        return Err(RuntimeProductionLifecycleErrorV2::RegistryMismatch);
    }
    if observed.finalizer_generation != permit.finalizer_generation() {
        return Err(RuntimeProductionLifecycleErrorV2::FinalizerGenerationMismatch);
    }
    RuntimeStartupRecoveryFixedPointProcessV2::writer_fence_is_bounded(
        observed.writer_fence_generation,
    )?;
    if !observed.writer_fence_open {
        return Err(RuntimeProductionLifecycleErrorV2::WriterFenceMismatch);
    }
    if !observed.maintenance_gate_closed {
        return Err(RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch);
    }
    Ok(())
}

pub struct RuntimeIngressOpenAcknowledgementObservationInputV2 {
    pub fence_generation: RuntimeWriterFenceGenerationV1,
    pub maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub resume_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub acknowledgement_revision: NonZeroU64,
    pub acknowledged_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl Debug for RuntimeIngressOpenAcknowledgementObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressOpenAcknowledgementObservationInputV2(<redacted>)")
    }
}

pub struct RuntimeIngressOpenAcknowledgementObservationV2 {
    input: RuntimeIngressOpenAcknowledgementObservationInputV2,
}

impl RuntimeIngressOpenAcknowledgementObservationV2 {
    pub fn new(input: RuntimeIngressOpenAcknowledgementObservationInputV2) -> Self {
        Self { input }
    }

    pub fn acknowledgement_revision(&self) -> NonZeroU64 {
        self.input.acknowledgement_revision
    }

    pub fn fence_generation(&self) -> RuntimeWriterFenceGenerationV1 {
        self.input.fence_generation
    }

    pub fn maintenance_gate_generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.input.maintenance_gate_generation
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.input.expires_at
    }
}

impl Debug for RuntimeIngressOpenAcknowledgementObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressOpenAcknowledgementObservationV2(<redacted>)")
    }
}

pub struct RuntimeOpenProductionRequestV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    writer_fence_generation: RuntimeWriterFenceGenerationV1,
    closed_maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    owner_revision: NonZeroU64,
    owner_expires_at: DateTime<Utc>,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
}

impl RuntimeOpenProductionRequestV2 {
    fn from_admission(state: &RuntimeAdmissionAcknowledgingProcessV2) -> Self {
        let owner = &state.resume.input.owner_receipt;
        Self {
            coordinator_generation: state.coordinator_generation,
            writer_fence_generation: state.resume.input.writer_fence_generation,
            closed_maintenance_gate_generation: state.resume.input.maintenance_gate_generation,
            owner_lease_id: owner.lease_id.clone(),
            owner_revision: owner.owner_revision,
            owner_expires_at: owner.expires_at,
            gateway_ready: state.resume.input.gateway_ready.clone(),
            registry_observation_sequence: state.resume.input.registry_observation_sequence,
            finalizer_generation: state.resume.input.finalizer_generation,
        }
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn writer_fence_generation(&self) -> RuntimeWriterFenceGenerationV1 {
        self.writer_fence_generation
    }

    pub fn closed_maintenance_gate_generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.closed_maintenance_gate_generation
    }

    pub fn owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.owner_lease_id
    }

    pub fn owner_revision(&self) -> NonZeroU64 {
        self.owner_revision
    }

    pub fn owner_expires_at(&self) -> DateTime<Utc> {
        self.owner_expires_at
    }

    pub fn gateway_ready(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.gateway_ready
    }

    pub fn registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.finalizer_generation
    }
}

impl Debug for RuntimeOpenProductionRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeOpenProductionRequestV2(<redacted>)")
    }
}

pub struct RuntimeOpenProductionObservationInputV2 {
    pub coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    pub writer_fence_generation: RuntimeWriterFenceGenerationV1,
    pub writer_fence_open: bool,
    pub maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    pub maintenance_gate_open: bool,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub readiness: RuntimeCapabilityReadinessSetV2,
    pub gateway_ready: RuntimeGatewayReadyAttestationV2,
    pub registry_empty: RuntimeRegistryRecoveryEmptyObservationV2,
    pub finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    pub finalizer_accepting: bool,
    pub supervisors_running: bool,
    pub observed_database_now: DateTime<Utc>,
    pub ingress_acknowledgement: RuntimeIngressOpenAcknowledgementObservationV2,
}

impl Debug for RuntimeOpenProductionObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeOpenProductionObservationInputV2(<redacted>)")
    }
}

pub struct RuntimeOpenProductionObservationV2 {
    input: RuntimeOpenProductionObservationInputV2,
}

impl RuntimeOpenProductionObservationV2 {
    pub fn new(input: RuntimeOpenProductionObservationInputV2) -> Self {
        Self { input }
    }
}

impl Debug for RuntimeOpenProductionObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeOpenProductionObservationV2(<redacted>)")
    }
}

pub trait RuntimeOpenProductionObservationPortV2 {
    type Error;

    fn observe_open_production(
        &self,
        request: &RuntimeOpenProductionRequestV2,
    ) -> Result<RuntimeOpenProductionObservationV2, Self::Error>;
}

pub struct RuntimeEmptyOpenEpochV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    gateway_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    ingress_acknowledgement: RuntimeIngressOpenAcknowledgementObservationV2,
    registry_empty: RuntimeRegistryRecoveryEmptyObservationV2,
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
}

impl RuntimeEmptyOpenEpochV2 {
    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.gateway_owner.lease_id.process_instance_id
    }

    pub fn gateway_ready(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.gateway_ready
    }

    pub fn ingress_acknowledgement(&self) -> &RuntimeIngressOpenAcknowledgementObservationV2 {
        &self.ingress_acknowledgement
    }

    pub fn registry_empty(&self) -> &RuntimeRegistryRecoveryEmptyObservationV2 {
        &self.registry_empty
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.finalizer_generation
    }

    pub fn readiness(&self) -> &RuntimeCapabilityReadinessSetV2 {
        &self.readiness
    }
}

impl Debug for RuntimeEmptyOpenEpochV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptyOpenEpochV2(<redacted>)")
    }
}

pub struct RuntimeEmptyOpenProcessV2 {
    pub(super) _admission: RuntimeAdmissionAcknowledgingProcessV2,
    epoch: RuntimeEmptyOpenEpochV2,
}

impl RuntimeEmptyOpenProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::OpenProduction
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.epoch.coordinator_generation()
    }

    pub fn epoch(&self) -> &RuntimeEmptyOpenEpochV2 {
        &self.epoch
    }
}

impl Debug for RuntimeEmptyOpenProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptyOpenProcessV2(<redacted>)")
    }
}

impl RuntimeAdmissionAcknowledgingProcessV2 {
    pub fn observe_open_production<P>(
        self,
        port: &P,
    ) -> Result<RuntimeEmptyOpenProcessV2, RuntimeProductionTransitionFailureV2<Self, P::Error>>
    where
        P: RuntimeOpenProductionObservationPortV2,
    {
        let request = RuntimeOpenProductionRequestV2::from_admission(&self);
        let observation = match port.observe_open_production(&request) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeProductionTransitionFailureV2::port(self, error));
            }
        };
        if let Err(error) = validate_open(&self, &request, &observation) {
            return Err(RuntimeProductionTransitionFailureV2::contract(self, error));
        }
        let RuntimeOpenProductionObservationInputV2 {
            owner_receipt,
            readiness,
            gateway_ready,
            registry_empty,
            finalizer_generation,
            ingress_acknowledgement,
            ..
        } = observation.input;
        let epoch = RuntimeEmptyOpenEpochV2 {
            coordinator_generation: self.coordinator_generation,
            gateway_owner: owner_receipt,
            readiness,
            gateway_ready,
            ingress_acknowledgement,
            registry_empty,
            finalizer_generation,
        };
        Ok(RuntimeEmptyOpenProcessV2 {
            _admission: self,
            epoch,
        })
    }
}

fn validate_open(
    state: &RuntimeAdmissionAcknowledgingProcessV2,
    request: &RuntimeOpenProductionRequestV2,
    observation: &RuntimeOpenProductionObservationV2,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    let observed = &observation.input;
    let acknowledgement = &observed.ingress_acknowledgement.input;
    if observed.coordinator_generation != request.coordinator_generation {
        return Err(RuntimeProductionLifecycleErrorV2::StaleGeneration);
    }
    if !observed.writer_fence_open
        || observed.writer_fence_generation != request.writer_fence_generation
    {
        return Err(RuntimeProductionLifecycleErrorV2::WriterFenceMismatch);
    }
    let expected_gate_generation = request
        .closed_maintenance_gate_generation
        .get()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeProductionLifecycleErrorV2::GenerationOverflow)?;
    if !observed.maintenance_gate_open
        || observed.maintenance_gate_generation.get() != expected_gate_generation.get()
    {
        return Err(RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch);
    }
    if !same_owner(&state.resume.input.owner_receipt, &observed.owner_receipt)
        || observed.owner_receipt.database_now != observed.observed_database_now
        || observed.owner_receipt.database_now < state.resume.input.owner_receipt.database_now
        || observed.owner_receipt.database_lease_duration().is_none()
    {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    if observed.readiness != state.resume.input.readiness
        && (!state
            .resume
            .input
            .readiness
            .has_same_authority_as(&observed.readiness)
            || !observed
                .readiness
                .has_strictly_newer_checks_than(&state.resume.input.readiness))
    {
        return Err(RuntimeProductionLifecycleErrorV2::ReadinessMismatch);
    }
    if observed.gateway_ready != request.gateway_ready {
        if observed.gateway_ready.connection_epoch != request.gateway_ready.connection_epoch {
            return Err(RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch);
        }
        if observed.gateway_ready.admission_revision != request.gateway_ready.admission_revision {
            return Err(RuntimeProductionLifecycleErrorV2::StaleAdmissionRevision);
        }
        return Err(RuntimeProductionLifecycleErrorV2::GatewayReadyMismatch);
    }
    if observed.registry_empty.process_instance_id() != &request.gateway_ready.process_instance_id
        || observed.registry_empty.observation_sequence() < request.registry_observation_sequence
    {
        return Err(RuntimeProductionLifecycleErrorV2::RegistryMismatch);
    }
    if observed.finalizer_generation != request.finalizer_generation
        || !observed.finalizer_accepting
    {
        return Err(RuntimeProductionLifecycleErrorV2::FinalizerGenerationMismatch);
    }
    if !observed.supervisors_running {
        return Err(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady);
    }
    if acknowledgement.fence_generation != request.writer_fence_generation
        || acknowledgement.maintenance_gate_generation != observed.maintenance_gate_generation
        || acknowledgement.gateway_owner_lease_id != request.owner_lease_id
        || acknowledgement.observed_owner_revision != request.owner_revision
        || acknowledgement.process_instance_id != request.gateway_ready.process_instance_id
        || acknowledgement.connection_epoch != request.gateway_ready.connection_epoch
        || acknowledgement.admission_revision != request.gateway_ready.admission_revision
        || acknowledgement.connected_event_sequence
            != request.gateway_ready.connected_event_sequence
        || acknowledgement.resume_sequence != request.gateway_ready.resume_sequence
    {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
    }
    bounded_generation(acknowledgement.acknowledgement_revision)?;
    if acknowledgement.acknowledged_at > observed.observed_database_now
        || observed.observed_database_now >= acknowledgement.expires_at
        || acknowledgement.expires_at > observed.owner_receipt.expires_at
    {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementNotCurrent);
    }
    Ok(())
}
