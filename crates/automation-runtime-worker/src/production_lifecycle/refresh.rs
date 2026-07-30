use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyAttestationV2,
    RuntimeIngressOpenAcknowledgementLeaseDurationV2, RuntimeIngressOpenAcknowledgementReceiptV2,
    RuntimeIngressOpenAcknowledgementV2, RuntimePublishIngressOpenAcknowledgementInputV2,
    RuntimePublishIngressOpenAcknowledgementV2, RuntimeWriterFenceGenerationV1,
};

use super::admission::RuntimeServingGatewayReadyRefreshV3;
use super::{
    RuntimeEmptyOpenProcessV2, RuntimeIngressOpenAcknowledgementObservationV2,
    RuntimeMaintenanceGateGenerationV2, RuntimeMutationFinalizerGenerationV1,
    RuntimeProductionLifecycleErrorV2, RuntimeServingOpenProcessV2,
};
use crate::{
    RuntimeAcceptedIngressOpenAcknowledgementV2, RuntimeAuthorizedIngressOpenAcknowledgementV2,
    RuntimeCapabilityReadinessSetV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2,
    RuntimeIngressOpenAcknowledgementPredecessorV2,
    RuntimeIngressOpenAcknowledgementSingleFlightV2, RuntimeRegistryRecoveryEmptyObservationV2,
};

pub struct RuntimeServingOpenBarrierCompletionAuthorityV3 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    final_receipt: RuntimeIngressOpenAcknowledgementReceiptV2,
}

impl RuntimeServingOpenBarrierCompletionAuthorityV3 {
    pub fn coordinator_generation_v3(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn gateway_ready_v3(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.gateway_ready
    }

    pub fn acknowledgement_v3(&self) -> &RuntimeIngressOpenAcknowledgementV2 {
        self.final_receipt.acknowledgement()
    }

    pub fn accepts_final_reobservation_v3(
        &self,
        receipt: &RuntimeIngressOpenAcknowledgementReceiptV2,
    ) -> bool {
        &self.final_receipt == receipt
    }
}

impl Debug for RuntimeServingOpenBarrierCompletionAuthorityV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingOpenBarrierCompletionAuthorityV3(<redacted>)")
    }
}

impl RuntimeServingOpenProcessV2 {
    pub fn authorize_ordinary_barrier_completion_v3(
        &self,
        final_receipt: &RuntimeIngressOpenAcknowledgementReceiptV2,
    ) -> Result<RuntimeServingOpenBarrierCompletionAuthorityV3, RuntimeProductionLifecycleErrorV2>
    {
        let epoch = self.epoch();
        if !epoch
            .ingress_acknowledgement()
            .accepts_exact_reobservation_v3(final_receipt)
        {
            return Err(RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch);
        }
        let acknowledgement = final_receipt.acknowledgement();
        let gateway_ready = epoch.gateway_ready();
        if !gateway_ready.was_explicitly_resumed()
            || acknowledgement.process_instance_id() != epoch.process_instance_id()
            || acknowledgement.connection_epoch() != gateway_ready.connection_epoch
            || acknowledgement.admission_revision() != gateway_ready.admission_revision
            || acknowledgement.connected_event_sequence() != gateway_ready.connected_event_sequence
            || acknowledgement.resume_sequence() != gateway_ready.resume_sequence
        {
            return Err(RuntimeProductionLifecycleErrorV2::GatewayReadyMismatch);
        }
        if acknowledgement.gateway_owner_lease_id() != &epoch.gateway_owner().lease_id
            || acknowledgement.observed_owner_revision() != epoch.gateway_owner().owner_revision
            || final_receipt.observed_database_now() < epoch.gateway_owner().database_now
            || final_receipt.observed_database_now() >= epoch.gateway_owner().expires_at
        {
            return Err(RuntimeProductionLifecycleErrorV2::OwnerMismatch);
        }
        Ok(RuntimeServingOpenBarrierCompletionAuthorityV3 {
            coordinator_generation: epoch.coordinator_generation(),
            gateway_ready: gateway_ready.clone(),
            final_receipt: final_receipt.clone(),
        })
    }
}

pub(super) fn validate_serving_gateway_ready_refresh_v3(
    current: &RuntimeGatewayReadyAttestationV2,
    observed: &RuntimeGatewayReadyAttestationV2,
    transition: RuntimeServingGatewayReadyRefreshV3,
) -> Result<(), RuntimeProductionLifecycleErrorV2> {
    if observed.connection_epoch != current.connection_epoch {
        return Err(RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch);
    }
    let expected_admission_revision = current
        .admission_revision
        .get()
        .checked_add(transition.admission_revision_delta_v3())
        .filter(|revision| *revision <= i64::MAX as u64);
    if expected_admission_revision != Some(observed.admission_revision.get()) {
        return Err(RuntimeProductionLifecycleErrorV2::StaleAdmissionRevision);
    }
    let accepted = match transition {
        RuntimeServingGatewayReadyRefreshV3::Current => observed == current,
        RuntimeServingGatewayReadyRefreshV3::ResumedSuccessor => {
            observed.process_instance_id == current.process_instance_id
                && observed.kind == current.kind
                && observed.connected_event_sequence == current.connected_event_sequence
                && observed.resume_sequence > current.resume_sequence
                && observed.was_explicitly_resumed()
        }
    };
    if !accepted {
        return Err(RuntimeProductionLifecycleErrorV2::GatewayReadyMismatch);
    }
    Ok(())
}

pub struct RuntimeEmptyOpenAcknowledgementRefreshInputV2 {
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub readiness: RuntimeCapabilityReadinessSetV2,
    pub gateway_ready: RuntimeGatewayReadyAttestationV2,
    pub writer_fence_generation: RuntimeWriterFenceGenerationV1,
    pub writer_fence_open: bool,
    pub maintenance_gate_generation: RuntimeMaintenanceGateGenerationV2,
    pub maintenance_gate_open: bool,
    pub registry_empty: RuntimeRegistryRecoveryEmptyObservationV2,
    pub finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    pub finalizer_accepting: bool,
    pub supervisors_running: bool,
    pub predecessor: RuntimeIngressOpenAcknowledgementPredecessorV2,
    pub lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2,
}

impl Debug for RuntimeEmptyOpenAcknowledgementRefreshInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptyOpenAcknowledgementRefreshInputV2(<redacted>)")
    }
}

pub struct RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2 {
    state: Box<RuntimeEmptyOpenProcessV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

impl RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2 {
    pub fn error(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub fn into_state(self) -> RuntimeEmptyOpenProcessV2 {
        *self.state
    }
}

impl Debug for RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2(<redacted>)")
    }
}

pub struct RuntimeEmptyOpenAcknowledgementRefreshV2 {
    state: Box<RuntimeEmptyOpenProcessV2>,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    registry_empty: RuntimeRegistryRecoveryEmptyObservationV2,
    operation: RuntimeIngressOpenAcknowledgementSingleFlightV2,
}

impl RuntimeEmptyOpenAcknowledgementRefreshV2 {
    pub fn operation_mut(&mut self) -> &mut RuntimeIngressOpenAcknowledgementSingleFlightV2 {
        &mut self.operation
    }

    pub fn request(&self) -> &RuntimePublishIngressOpenAcknowledgementV2 {
        self.operation.request()
    }

    pub fn complete(
        self,
        accepted: RuntimeAcceptedIngressOpenAcknowledgementV2,
    ) -> Result<RuntimeEmptyOpenProcessV2, RuntimeEmptyOpenAcknowledgementRefreshCompletionFailureV2>
    {
        if accepted.request() != self.operation.request() {
            return Err(RuntimeEmptyOpenAcknowledgementRefreshCompletionFailureV2 {
                refresh: Box::new(self),
                error: RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch,
            });
        }
        let RuntimeEmptyOpenAcknowledgementRefreshV2 {
            state,
            owner_receipt,
            readiness,
            registry_empty,
            ..
        } = self;
        let RuntimeEmptyOpenProcessV2 {
            _admission,
            mut epoch,
        } = *state;
        epoch.gateway_owner = owner_receipt;
        epoch.readiness = readiness;
        epoch.ingress_acknowledgement =
            RuntimeIngressOpenAcknowledgementObservationV2::from_accepted(accepted);
        epoch.registry_empty = registry_empty;
        Ok(RuntimeEmptyOpenProcessV2 { _admission, epoch })
    }
}

impl Debug for RuntimeEmptyOpenAcknowledgementRefreshV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptyOpenAcknowledgementRefreshV2(<redacted>)")
    }
}

pub struct RuntimeEmptyOpenAcknowledgementRefreshCompletionFailureV2 {
    refresh: Box<RuntimeEmptyOpenAcknowledgementRefreshV2>,
    error: RuntimeProductionLifecycleErrorV2,
}

impl RuntimeEmptyOpenAcknowledgementRefreshCompletionFailureV2 {
    pub fn error(&self) -> RuntimeProductionLifecycleErrorV2 {
        self.error
    }

    pub fn into_refresh(self) -> RuntimeEmptyOpenAcknowledgementRefreshV2 {
        *self.refresh
    }
}

impl Debug for RuntimeEmptyOpenAcknowledgementRefreshCompletionFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptyOpenAcknowledgementRefreshCompletionFailureV2(<redacted>)")
    }
}

impl RuntimeEmptyOpenProcessV2 {
    pub fn authorize_ingress_open_acknowledgement_predecessor_observation(
        &self,
    ) -> RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2 {
        RuntimeIngressOpenAcknowledgementPredecessorObservationAuthorizationV2::for_shard(
            self.epoch.gateway_owner.lease_id.gateway_shard_id.clone(),
        )
    }

    pub fn authorize_ingress_open_acknowledgement_refresh(
        self,
        input: RuntimeEmptyOpenAcknowledgementRefreshInputV2,
    ) -> Result<
        RuntimeEmptyOpenAcknowledgementRefreshV2,
        RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2,
    > {
        if let Err(error) = validate_refresh(&self, &input) {
            return Err(
                RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2 {
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
                gateway_ready: input.gateway_ready,
                lease_for: input.lease_for,
            },
        ) {
            Ok(request) => request,
            Err(_) => {
                return Err(
                    RuntimeEmptyOpenAcknowledgementRefreshAuthorizationFailureV2 {
                        state: Box::new(self),
                        error: RuntimeProductionLifecycleErrorV2::IngressAcknowledgementMismatch,
                    },
                );
            }
        };
        Ok(RuntimeEmptyOpenAcknowledgementRefreshV2 {
            state: Box::new(self),
            owner_receipt: input.owner_receipt,
            readiness: input.readiness,
            registry_empty: input.registry_empty,
            operation: RuntimeIngressOpenAcknowledgementSingleFlightV2::new(
                RuntimeAuthorizedIngressOpenAcknowledgementV2::from_request(
                    request,
                    input.predecessor,
                ),
            ),
        })
    }
}

fn validate_refresh(
    state: &RuntimeEmptyOpenProcessV2,
    input: &RuntimeEmptyOpenAcknowledgementRefreshInputV2,
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
    if input.gateway_ready != epoch.gateway_ready {
        if input.gateway_ready.connection_epoch != epoch.gateway_ready.connection_epoch {
            return Err(RuntimeProductionLifecycleErrorV2::StaleConnectionEpoch);
        }
        if input.gateway_ready.admission_revision != epoch.gateway_ready.admission_revision {
            return Err(RuntimeProductionLifecycleErrorV2::StaleAdmissionRevision);
        }
        return Err(RuntimeProductionLifecycleErrorV2::GatewayReadyMismatch);
    }
    let current_acknowledgement = epoch.ingress_acknowledgement.acknowledgement();
    if !input.writer_fence_open
        || input.writer_fence_generation != current_acknowledgement.fence_generation()
    {
        return Err(RuntimeProductionLifecycleErrorV2::WriterFenceMismatch);
    }
    if !input.maintenance_gate_open
        || input.maintenance_gate_generation.get()
            != current_acknowledgement.maintenance_gate_generation().get()
    {
        return Err(RuntimeProductionLifecycleErrorV2::MaintenanceGateMismatch);
    }
    if input.registry_empty.process_instance_id() != epoch.registry_empty.process_instance_id()
        || input.registry_empty.observation_sequence() < epoch.registry_empty.observation_sequence()
        || input.registry_empty.retained_slot_count() != epoch.registry_empty.retained_slot_count()
        || input.registry_empty.retained_empty_tombstone_count()
            != epoch.registry_empty.retained_empty_tombstone_count()
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
