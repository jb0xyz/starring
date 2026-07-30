use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyAttestationV2,
    RuntimeIngressOpenAcknowledgementLeaseDurationV2,
    RuntimeIngressOpenAcknowledgementRequestDigestV2, RuntimeIngressOpenAcknowledgementV2,
    RuntimePublishIngressOpenAcknowledgementInputV2, RuntimePublishIngressOpenAcknowledgementV2,
    RuntimeServingSlotV2, RuntimeWriterFenceGenerationV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use super::admission::RuntimeServingGatewayReadyRefreshV3;
use super::refresh::validate_serving_gateway_ready_refresh_v3;
use super::{
    RuntimeAdmissionAcknowledgingProcessV2, RuntimeEmptyOpenEpochV2, RuntimeEmptyOpenProcessV2,
    RuntimeIngressOpenAcknowledgementObservationV2, RuntimeMaintenanceGateGenerationV2,
    RuntimeMutationFinalizerGenerationV1, RuntimeProductionLifecycleErrorV2,
    RuntimeProductionLifecycleStageV2, RuntimeProductionTransitionFailureV2,
    RuntimeShutdownCauseV2, RuntimeShuttingDownProcessV2,
};
use crate::{
    RuntimeAcceptedIngressOpenAcknowledgementV2, RuntimeAuthorizedIngressOpenAcknowledgementV2,
    RuntimeCapabilityReadinessSetV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2,
    RuntimeIngressOpenAcknowledgementPredecessorV2,
    RuntimeIngressOpenAcknowledgementSingleFlightV2, RuntimeRegistryGlobalObservationSequenceV2,
};

mod route_set;
mod slot_work;

pub use route_set::{
    accept_runtime_route_set_observation_v2, RuntimeRouteSetObservationErrorV2,
    RuntimeRouteSetObservationInputV2, RuntimeRouteSetObservationV2,
};
use slot_work::RuntimeServingSlotWorkSupervisorV2;
#[cfg(test)]
pub(crate) use slot_work::{
    runtime_serving_slot_work_test_authority_v2, RuntimeServingSlotWorkTestHandleV2,
};
pub use slot_work::{
    RuntimeServingOpenSupervisorConfigErrorV2, RuntimeServingOpenSupervisorConfigV2,
    RuntimeServingSlotWorkErrorV2, RuntimeServingSlotWorkPermitV2, RuntimeServingSlotWorkRequestV2,
};

#[derive(PartialEq, Eq)]
pub struct RuntimeRouteSetEpochV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    initial_retained_slot_count: u64,
    initial_retained_empty_tombstone_count: u64,
}

impl RuntimeRouteSetEpochV2 {
    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn initial_registry_observation_sequence(
        &self,
    ) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.initial_registry_observation_sequence
    }

    pub fn initial_retained_slot_count(&self) -> u64 {
        self.initial_retained_slot_count
    }

    pub fn initial_retained_empty_tombstone_count(&self) -> u64 {
        self.initial_retained_empty_tombstone_count
    }
}

impl Debug for RuntimeRouteSetEpochV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteSetEpochV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenRequestV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    gateway_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    ingress_acknowledgement_source_revision: Option<NonZeroU64>,
    ingress_acknowledgement_request_digest: RuntimeIngressOpenAcknowledgementRequestDigestV2,
    ingress_acknowledgement: RuntimeIngressOpenAcknowledgementV2,
    ingress_acknowledgement_revision: NonZeroU64,
    writer_fence_generation: RuntimeWriterFenceGenerationV1,
    maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    ingress_acknowledgement_expires_at: DateTime<Utc>,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    route_set_epoch: RuntimeRouteSetEpochV2,
}

impl RuntimeServingOpenRequestV2 {
    fn from_empty_open(state: &RuntimeEmptyOpenProcessV2) -> Self {
        let epoch = state.epoch();
        let acknowledgement = epoch.ingress_acknowledgement().acknowledgement();
        let acknowledgement_request = epoch.ingress_acknowledgement().accepted_request();
        let route_set_epoch = RuntimeRouteSetEpochV2 {
            coordinator_generation: epoch.coordinator_generation(),
            process_instance_id: epoch.process_instance_id().clone(),
            initial_registry_observation_sequence: epoch.registry_empty().observation_sequence(),
            initial_retained_slot_count: epoch.registry_empty().retained_slot_count(),
            initial_retained_empty_tombstone_count: epoch
                .registry_empty()
                .retained_empty_tombstone_count(),
        };
        Self {
            coordinator_generation: epoch.coordinator_generation(),
            process_instance_id: epoch.process_instance_id().clone(),
            gateway_owner: epoch.gateway_owner().clone(),
            readiness: epoch.readiness().clone(),
            gateway_ready: epoch.gateway_ready().clone(),
            ingress_acknowledgement_source_revision: acknowledgement_request
                .source_acknowledgement_revision(),
            ingress_acknowledgement_request_digest: acknowledgement_request.request_digest(),
            ingress_acknowledgement: acknowledgement.clone(),
            ingress_acknowledgement_revision: acknowledgement.acknowledgement_revision(),
            writer_fence_generation: acknowledgement.fence_generation(),
            maintenance_gate_generation: epoch
                .ingress_acknowledgement()
                .maintenance_gate_generation(),
            ingress_acknowledgement_expires_at: acknowledgement.expires_at(),
            registry_observation_sequence: epoch.registry_empty().observation_sequence(),
            finalizer_generation: epoch.finalizer_generation(),
            route_set_epoch,
        }
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn gateway_owner(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.gateway_owner
    }

    pub fn readiness(&self) -> &RuntimeCapabilityReadinessSetV2 {
        &self.readiness
    }

    pub fn gateway_ready(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.gateway_ready
    }

    pub fn ingress_acknowledgement_revision(&self) -> NonZeroU64 {
        self.ingress_acknowledgement_revision
    }

    pub fn writer_fence_generation(&self) -> RuntimeWriterFenceGenerationV1 {
        self.writer_fence_generation
    }

    pub fn maintenance_gate_generation(&self) -> RuntimeMaintenanceGateGenerationV2 {
        self.maintenance_gate_generation
    }

    pub fn ingress_acknowledgement_expires_at(&self) -> DateTime<Utc> {
        self.ingress_acknowledgement_expires_at
    }

    pub fn registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.finalizer_generation
    }

    pub fn route_set_epoch(&self) -> &RuntimeRouteSetEpochV2 {
        &self.route_set_epoch
    }
}

impl Debug for RuntimeServingOpenRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenRequestV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenObservationInputV2 {
    pub coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    pub process_instance_id: ProcessInstanceId,
    pub gateway_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    pub readiness: RuntimeCapabilityReadinessSetV2,
    pub gateway_ready: RuntimeGatewayReadyAttestationV2,
    pub ingress_acknowledgement_revision: NonZeroU64,
    pub writer_fence_generation: RuntimeWriterFenceGenerationV1,
    pub writer_fence_open: bool,
    pub maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    pub maintenance_gate_open: bool,
    pub ingress_acknowledgement_expires_at: DateTime<Utc>,
    pub observed_database_now: DateTime<Utc>,
    pub ingress_acknowledgement_predecessor: RuntimeIngressOpenAcknowledgementPredecessorV2,
    pub finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    pub finalizer_accepting: bool,
    pub route_set_epoch_coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    pub route_set_epoch_process_instance_id: ProcessInstanceId,
    pub route_set_epoch_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    pub route_set: RuntimeRouteSetObservationV2,
    pub supervisors_running: bool,
}

impl Debug for RuntimeServingOpenObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenObservationInputV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenObservationV2 {
    input: RuntimeServingOpenObservationInputV2,
}

impl RuntimeServingOpenObservationV2 {
    pub fn new(input: RuntimeServingOpenObservationInputV2) -> Self {
        Self { input }
    }
}

impl Debug for RuntimeServingOpenObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenObservationV2(<redacted>)")
    }
}

pub trait RuntimeServingOpenObservationPortV2 {
    type Error;

    fn observe_serving_open(
        &self,
        request: &RuntimeServingOpenRequestV2,
    ) -> Result<RuntimeServingOpenObservationV2, Self::Error>;
}

pub struct RuntimeServingOpenPreparedV2 {
    state: Box<RuntimeEmptyOpenProcessV2>,
    gateway_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    route_set: RuntimeRouteSetObservationV2,
    route_set_epoch: RuntimeRouteSetEpochV2,
    supervisor: RuntimeServingSlotWorkSupervisorV2,
}

impl RuntimeServingOpenPreparedV2 {
    pub fn route_set_epoch(&self) -> &RuntimeRouteSetEpochV2 {
        &self.route_set_epoch
    }

    pub fn commit(self) -> RuntimeServingOpenProcessV2 {
        let Self {
            state,
            gateway_owner,
            readiness,
            route_set,
            route_set_epoch,
            supervisor,
        } = self;
        let RuntimeEmptyOpenProcessV2 { _admission, epoch } = *state;
        let RuntimeEmptyOpenEpochV2 {
            coordinator_generation,
            gateway_owner: _,
            readiness: _,
            gateway_ready,
            ingress_acknowledgement,
            registry_empty: _,
            finalizer_generation,
        } = epoch;
        RuntimeServingOpenProcessV2 {
            _admission,
            epoch: RuntimeServingOpenEpochV2 {
                coordinator_generation,
                gateway_owner,
                readiness,
                gateway_ready,
                ingress_acknowledgement,
                route_set_epoch,
                route_set,
                finalizer_generation,
            },
            supervisor,
        }
    }

    pub fn cancel(self) -> RuntimeEmptyOpenProcessV2 {
        *self.state
    }
}

impl Debug for RuntimeServingOpenPreparedV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenPreparedV2(<redacted>)")
    }
}

impl RuntimeEmptyOpenProcessV2 {
    pub fn prepare_serving_open<P>(
        self,
        port: &P,
        config: RuntimeServingOpenSupervisorConfigV2,
    ) -> Result<RuntimeServingOpenPreparedV2, RuntimeProductionTransitionFailureV2<Self, P::Error>>
    where
        P: RuntimeServingOpenObservationPortV2,
    {
        let request = RuntimeServingOpenRequestV2::from_empty_open(&self);
        let observation = match port.observe_serving_open(&request) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeProductionTransitionFailureV2::port(self, error));
            }
        };
        if let Err(error) = validate_serving_open(&request, &observation) {
            return Err(RuntimeProductionTransitionFailureV2::contract(self, error));
        }
        Ok(RuntimeServingOpenPreparedV2 {
            state: Box::new(self),
            gateway_owner: observation.input.gateway_owner,
            readiness: observation.input.readiness,
            route_set: observation.input.route_set,
            route_set_epoch: request.route_set_epoch,
            supervisor: RuntimeServingSlotWorkSupervisorV2::new(config),
        })
    }
}

fn validate_serving_open(
    request: &RuntimeServingOpenRequestV2,
    observation: &RuntimeServingOpenObservationV2,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    let observed = &observation.input;
    if observed.coordinator_generation != request.coordinator_generation
        || observed.route_set_epoch_coordinator_generation
            != request.route_set_epoch.coordinator_generation
    {
        return Err(RuntimeProductionLifecycleErrorV2::StaleGeneration);
    }
    if observed.process_instance_id != request.process_instance_id
        || observed.route_set_epoch_process_instance_id != request.process_instance_id
        || observed.route_set.process_instance_id() != &request.process_instance_id
    {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    let previous_owner = &request.gateway_owner;
    if observed.gateway_owner.lease_id != previous_owner.lease_id
        || observed.gateway_owner.owner_revision != previous_owner.owner_revision
        || observed.gateway_owner.expires_at != previous_owner.expires_at
        || observed.gateway_owner.database_now < previous_owner.database_now
        || observed.observed_database_now < observed.gateway_owner.database_now
        || observed.observed_database_now >= observed.gateway_owner.expires_at
        || observed.gateway_owner.database_lease_duration().is_none()
    {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    if observed.readiness != request.readiness
        && (!request.readiness.has_same_authority_as(&observed.readiness)
            || !observed
                .readiness
                .has_strictly_newer_checks_than(&request.readiness))
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
    if observed.ingress_acknowledgement_revision != request.ingress_acknowledgement_revision
        || observed.ingress_acknowledgement_expires_at != request.ingress_acknowledgement_expires_at
    {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
    }
    if !observed.writer_fence_open
        || observed.writer_fence_generation != request.writer_fence_generation
    {
        return Err(RuntimeProductionLifecycleErrorV2::WriterFenceMismatch);
    }
    if !observed.maintenance_gate_open
        || observed.maintenance_gate_generation != request.maintenance_gate_generation
    {
        return Err(RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch);
    }
    if observed.observed_database_now >= observed.ingress_acknowledgement_expires_at {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementNotCurrent);
    }
    if observed.finalizer_generation != request.finalizer_generation
        || !observed.finalizer_accepting
    {
        return Err(RuntimeProductionLifecycleErrorV2::FinalizerGenerationMismatch);
    }
    if !observed.supervisors_running {
        return Err(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady);
    }
    if observed.route_set_epoch_registry_observation_sequence
        != request
            .route_set_epoch
            .initial_registry_observation_sequence
        || observed.route_set.observation_sequence() != request.registry_observation_sequence
        || observed.route_set.retained_slot_count()
            != request.route_set_epoch.initial_retained_slot_count
        || observed.route_set.retained_empty_tombstone_count()
            != request
                .route_set_epoch
                .initial_retained_empty_tombstone_count
        || !observed.route_set.is_empty()
    {
        return Err(RuntimeProductionLifecycleErrorV2::RegistryMismatch);
    }
    if observed
        .ingress_acknowledgement_predecessor
        .gateway_shard_id()
        != &previous_owner.lease_id.gateway_shard_id
    {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
    }
    let Some(predecessor_receipt) = observed
        .ingress_acknowledgement_predecessor
        .present_receipt()
    else {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
    };
    if predecessor_receipt.source_acknowledgement_revision()
        != request.ingress_acknowledgement_source_revision
        || predecessor_receipt.request_digest() != request.ingress_acknowledgement_request_digest
        || predecessor_receipt.acknowledgement() != &request.ingress_acknowledgement
        || observed
            .ingress_acknowledgement_predecessor
            .observed_database_now()
            < observed.gateway_owner.database_now
        || observed
            .ingress_acknowledgement_predecessor
            .observed_database_now()
            >= request.ingress_acknowledgement_expires_at
    {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementNotCurrent);
    }
    Ok(())
}

pub struct RuntimeServingOpenEpochV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    gateway_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    ingress_acknowledgement: RuntimeIngressOpenAcknowledgementObservationV2,
    route_set_epoch: RuntimeRouteSetEpochV2,
    route_set: RuntimeRouteSetObservationV2,
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
}

impl RuntimeServingOpenEpochV2 {
    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        self.route_set_epoch.process_instance_id()
    }

    pub fn gateway_owner(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.gateway_owner
    }

    pub fn gateway_ready(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.gateway_ready
    }

    pub fn ingress_acknowledgement(&self) -> &RuntimeIngressOpenAcknowledgementObservationV2 {
        &self.ingress_acknowledgement
    }

    pub fn route_set_epoch(&self) -> &RuntimeRouteSetEpochV2 {
        &self.route_set_epoch
    }

    pub fn route_set(&self) -> &RuntimeRouteSetObservationV2 {
        &self.route_set
    }

    pub fn finalizer_generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.finalizer_generation
    }

    pub fn readiness(&self) -> &RuntimeCapabilityReadinessSetV2 {
        &self.readiness
    }
}

impl Debug for RuntimeServingOpenEpochV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenEpochV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenProcessV2 {
    _admission: RuntimeAdmissionAcknowledgingProcessV2,
    epoch: RuntimeServingOpenEpochV2,
    supervisor: RuntimeServingSlotWorkSupervisorV2,
}

impl RuntimeServingOpenProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::OpenProduction
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.epoch.coordinator_generation()
    }

    pub fn epoch(&self) -> &RuntimeServingOpenEpochV2 {
        &self.epoch
    }

    pub fn active_slot_work_count(&self) -> usize {
        self.supervisor.active_count()
    }

    pub fn authorize_slot_work(
        &self,
        slot: RuntimeServingSlotV2,
    ) -> RuntimeServingSlotWorkRequestV2 {
        RuntimeServingSlotWorkRequestV2::new(
            &self.epoch.route_set_epoch,
            self.epoch.route_set.observation_sequence(),
            slot,
        )
    }

    pub fn begin_slot_work(
        &mut self,
        request: RuntimeServingSlotWorkRequestV2,
    ) -> Result<RuntimeServingSlotWorkPermitV2, RuntimeServingSlotWorkErrorV2> {
        let route_set_sequence = self.epoch.route_set.observation_sequence();
        let slot = request.into_slot(&self.epoch.route_set_epoch, route_set_sequence)?;
        self.supervisor
            .begin(&self.epoch.route_set_epoch, route_set_sequence, slot)
    }

    pub fn complete_slot_work(
        &mut self,
        permit: RuntimeServingSlotWorkPermitV2,
    ) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        self.supervisor
            .complete(&self.epoch.route_set_epoch, permit)
    }

    pub(super) fn seal_slot_work(&mut self) {
        self.supervisor.seal();
    }
}

impl Debug for RuntimeServingOpenProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenProcessV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenAcknowledgementRefreshInputV2 {
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub readiness: RuntimeCapabilityReadinessSetV2,
    pub gateway_ready: RuntimeGatewayReadyAttestationV2,
    pub writer_fence_generation: RuntimeWriterFenceGenerationV1,
    pub writer_fence_open: bool,
    pub maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    pub maintenance_gate_open: bool,
    pub maintenance_gate_opening: bool,
    pub route_set: RuntimeRouteSetObservationV2,
    pub finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    pub finalizer_accepting: bool,
    pub supervisors_running: bool,
    pub predecessor: RuntimeIngressOpenAcknowledgementPredecessorV2,
    pub lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2,
}

impl Debug for RuntimeServingOpenAcknowledgementRefreshInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenAcknowledgementRefreshInputV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2 {
    state: Box<RuntimeServingOpenProcessV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

impl RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2 {
    pub fn error(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub fn into_state(self) -> RuntimeServingOpenProcessV2 {
        *self.state
    }
}

impl Debug for RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenAcknowledgementRefreshV2 {
    state: Box<RuntimeServingOpenProcessV2>,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    route_set: RuntimeRouteSetObservationV2,
    operation: RuntimeIngressOpenAcknowledgementSingleFlightV2,
}

impl RuntimeServingOpenAcknowledgementRefreshV2 {
    pub fn route_set_epoch(&self) -> &RuntimeRouteSetEpochV2 {
        self.state.epoch().route_set_epoch()
    }

    pub fn operation_mut(&mut self) -> &mut RuntimeIngressOpenAcknowledgementSingleFlightV2 {
        &mut self.operation
    }

    pub fn request(&self) -> &RuntimePublishIngressOpenAcknowledgementV2 {
        self.operation.request()
    }

    pub fn begin_shutdown(self, cause: RuntimeShutdownCauseV2) -> RuntimeShuttingDownProcessV2 {
        let Self { state, .. } = self;
        let generation = state.coordinator_generation();
        match state.begin_shutdown(generation, cause) {
            Ok(shutdown) => shutdown,
            Err(_) => unreachable!("current serving generation must authorize shutdown"),
        }
    }

    pub fn complete(
        self,
        accepted: RuntimeAcceptedIngressOpenAcknowledgementV2,
    ) -> Result<
        RuntimeServingOpenProcessV2,
        RuntimeServingOpenAcknowledgementRefreshCompletionFailureV2,
    > {
        if accepted.request() != self.operation.request() {
            return Err(
                RuntimeServingOpenAcknowledgementRefreshCompletionFailureV2 {
                    refresh: Box::new(self),
                    error: RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch,
                },
            );
        }
        let Self {
            state,
            owner_receipt,
            readiness,
            gateway_ready,
            route_set,
            ..
        } = self;
        let RuntimeServingOpenProcessV2 {
            _admission,
            mut epoch,
            supervisor,
        } = *state;
        epoch.gateway_owner = owner_receipt;
        epoch.readiness = readiness;
        epoch.gateway_ready = gateway_ready;
        epoch.ingress_acknowledgement =
            RuntimeIngressOpenAcknowledgementObservationV2::from_accepted(accepted);
        epoch.route_set = route_set;
        Ok(RuntimeServingOpenProcessV2 {
            _admission,
            epoch,
            supervisor,
        })
    }
}

impl Debug for RuntimeServingOpenAcknowledgementRefreshV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenAcknowledgementRefreshV2(<redacted>)")
    }
}

pub struct RuntimeServingOpenAcknowledgementRefreshCompletionFailureV2 {
    refresh: Box<RuntimeServingOpenAcknowledgementRefreshV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

impl RuntimeServingOpenAcknowledgementRefreshCompletionFailureV2 {
    pub fn error(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub fn into_refresh(self) -> RuntimeServingOpenAcknowledgementRefreshV2 {
        *self.refresh
    }
}

impl Debug for RuntimeServingOpenAcknowledgementRefreshCompletionFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("RuntimeServingOpenAcknowledgementRefreshCompletionFailureV2(<redacted>)")
    }
}

impl RuntimeServingOpenProcessV2 {
    pub fn authorize_ingress_open_acknowledgement_predecessor_observation(
        &self,
    ) -> RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2 {
        RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2::for_shard(
            self.epoch.gateway_owner.lease_id.gateway_shard_id.clone(),
        )
    }

    pub fn authorize_ingress_open_acknowledgement_refresh(
        self,
        input: RuntimeServingOpenAcknowledgementRefreshInputV2,
    ) -> Result<
        RuntimeServingOpenAcknowledgementRefreshV2,
        RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2,
    > {
        self.authorize_ingress_open_acknowledgement_refresh_with_gateway_transition_v3(
            input,
            RuntimeServingGatewayReadyRefreshV3::Current,
        )
    }

    pub fn authorize_resumed_ingress_open_acknowledgement_refresh_v3(
        self,
        input: RuntimeServingOpenAcknowledgementRefreshInputV2,
    ) -> Result<
        RuntimeServingOpenAcknowledgementRefreshV2,
        RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2,
    > {
        self.authorize_ingress_open_acknowledgement_refresh_with_gateway_transition_v3(
            input,
            RuntimeServingGatewayReadyRefreshV3::ResumedSuccessor,
        )
    }

    fn authorize_ingress_open_acknowledgement_refresh_with_gateway_transition_v3(
        self,
        input: RuntimeServingOpenAcknowledgementRefreshInputV2,
        gateway_transition: RuntimeServingGatewayReadyRefreshV3,
    ) -> Result<
        RuntimeServingOpenAcknowledgementRefreshV2,
        RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2,
    > {
        if let Err(error) = validate_serving_refresh(&self, &input, gateway_transition) {
            return Err(
                RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2 {
                    state: Box::new(self),
                    error,
                },
            );
        }
        let request = match RuntimePublishIngressOpenAcknowledgementV2::new(
            RuntimePublishIngressOpenAcknowledgementInputV2 {
                source_acknowledgement_revision: input
                    .predecessor
                    .source_acknowledgement_revision(),
                fence_generation: input.writer_fence_generation,
                maintenance_gate_generation: NonZeroU64::new(
                    input.maintenance_gate_generation.get(),
                )
                .expect("validated maintenance gate generation must remain nonzero"),
                owner_receipt: input.owner_receipt.clone(),
                gateway_ready: input.gateway_ready.clone(),
                lease_for: input.lease_for,
            },
        ) {
            Ok(request) => request,
            Err(_) => {
                return Err(
                    RuntimeServingOpenAcknowledgementRefreshAuthorizationFailureV2 {
                        state: Box::new(self),
                        error: RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch,
                    },
                );
            }
        };
        Ok(RuntimeServingOpenAcknowledgementRefreshV2 {
            state: Box::new(self),
            owner_receipt: input.owner_receipt,
            readiness: input.readiness,
            gateway_ready: input.gateway_ready,
            route_set: input.route_set,
            operation: RuntimeIngressOpenAcknowledgementSingleFlightV2::new(
                RuntimeAuthorizedIngressOpenAcknowledgementV2::from_request(
                    request,
                    input.predecessor,
                ),
            ),
        })
    }
}

fn validate_serving_refresh(
    state: &RuntimeServingOpenProcessV2,
    input: &RuntimeServingOpenAcknowledgementRefreshInputV2,
    gateway_transition: RuntimeServingGatewayReadyRefreshV3,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    let epoch = &state.epoch;
    let previous_owner = &epoch.gateway_owner;
    let successor_owner_revision = previous_owner
        .owner_revision
        .get()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeProductionLifecycleErrorV2::GenerationOverflow)?;
    if input.owner_receipt.lease_id != previous_owner.lease_id {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    let exact_current_owner = input.owner_receipt.owner_revision == previous_owner.owner_revision
        && input.owner_receipt.expires_at == previous_owner.expires_at;
    let exact_successor_owner = input.owner_receipt.owner_revision == successor_owner_revision
        && input.owner_receipt.expires_at > previous_owner.expires_at;
    if (!exact_current_owner && !exact_successor_owner)
        || input.owner_receipt.database_now <= previous_owner.database_now
        || input.owner_receipt.database_lease_duration().is_none()
    {
        return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
    }
    if input.readiness != epoch.readiness
        && (!epoch.readiness.has_same_authority_as(&input.readiness)
            || !input
                .readiness
                .has_strictly_newer_checks_than(&epoch.readiness))
    {
        return Err(RuntimeProductionLifecycleErrorV2::ReadinessMismatch);
    }
    validate_serving_gateway_ready_refresh_v3(
        &epoch.gateway_ready,
        &input.gateway_ready,
        gateway_transition,
    )?;
    let current_acknowledgement = epoch.ingress_acknowledgement.acknowledgement();
    if !input.writer_fence_open
        || input.writer_fence_generation != current_acknowledgement.fence_generation()
    {
        return Err(RuntimeProductionLifecycleErrorV2::WriterFenceMismatch);
    }
    let current_gate_generation = current_acknowledgement.maintenance_gate_generation().get();
    let current_open = input.maintenance_gate_open
        && !input.maintenance_gate_opening
        && input.maintenance_gate_generation.get() == current_gate_generation;
    let resumed_opening = !input.maintenance_gate_open
        && input.maintenance_gate_opening
        && current_gate_generation
            .checked_add(2)
            .filter(|generation| *generation <= i64::MAX as u64)
            == Some(input.maintenance_gate_generation.get());
    let maintenance_gate_matches = match gateway_transition {
        RuntimeServingGatewayReadyRefreshV3::Current => current_open,
        RuntimeServingGatewayReadyRefreshV3::ResumedSuccessor => current_open || resumed_opening,
    };
    if !maintenance_gate_matches {
        return Err(RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch);
    }
    let route_set_sequence = input.route_set.observation_sequence();
    let previous_route_set_sequence = epoch.route_set.observation_sequence();
    if input.route_set.process_instance_id() != epoch.route_set_epoch.process_instance_id()
        || route_set_sequence < previous_route_set_sequence
        || (route_set_sequence == previous_route_set_sequence && input.route_set != epoch.route_set)
    {
        return Err(RuntimeProductionLifecycleErrorV2::RegistryMismatch);
    }
    if input.finalizer_generation != epoch.finalizer_generation || !input.finalizer_accepting {
        return Err(RuntimeProductionLifecycleErrorV2::FinalizerGenerationMismatch);
    }
    if !input.supervisors_running {
        return Err(RuntimeProductionLifecycleErrorV2::SupervisorsNotReady);
    }
    if input.predecessor.gateway_shard_id() != &previous_owner.lease_id.gateway_shard_id {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
    }
    let Some(predecessor_receipt) = input.predecessor.present_receipt() else {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
    };
    if predecessor_receipt.acknowledgement() != current_acknowledgement
        || predecessor_receipt.request_digest()
            != epoch
                .ingress_acknowledgement
                .accepted_request()
                .request_digest()
        || input.predecessor.observed_database_now() < input.owner_receipt.database_now
        || input.predecessor.observed_database_now() >= current_acknowledgement.expires_at()
    {
        return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementNotCurrent);
    }
    Ok(())
}
