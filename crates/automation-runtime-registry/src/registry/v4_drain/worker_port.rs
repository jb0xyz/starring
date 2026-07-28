use automation_runtime_worker::{
    RuntimeAuthorizedRegistryRefenceEvidenceV4, RuntimeDurablePreviousProcessDrainTeardownV4,
    RuntimeDurableRefenceReceiptV4, RuntimeDurableRoutedClaimReceiptV4,
    RuntimeDurableSameProcessDrainAcknowledgementV4, RuntimePendingDrainRegistryTransitionPortV4,
    RuntimeRoutedDrainRollbackPermitV4, RuntimeRoutedDrainRollbackPortV4,
    RuntimeSelectedExpiredPreviousOwnerV4, RuntimeSelectedUnclaimedPendingDrainV4,
};

use super::*;

impl RuntimePendingDrainRegistryTransitionPortV4 for ServingSlotRegistryV1 {
    type Error = ServingSlotRegistryError;
    type RoutedObserved = RoutedObservedV4;
    type RoutedSealed = RoutedSealedV4;
    type RoutedClaimedSealed = RoutedClaimedSealedV4;
    type LocallyRefencedSealed = LocallyRefencedSealedV4;
    type DurablyRefencedSealed = DurablyRefencedSealedV4;
    type DrainingRefencedSealed = DrainingRefencedSealedV4;
    type RouteAbsentSealed = RouteAbsentSealedV4;
    type EmptySuccessionSealed = EmptySuccessionSealedV4;
    type AcknowledgedEmpty = AcknowledgedEmptyV4;

    fn seal_routed(
        &self,
        source: Self::RoutedObserved,
        authorization: &RuntimeSelectedUnclaimedPendingDrainV4,
    ) -> Result<(Self::RoutedSealed, RuntimeRoutedSealPortObservationV4), Self::Error> {
        let candidate = authorization.candidate();
        let key = ServingSlotKeyV1::new(
            candidate.slot().guild_id,
            candidate.slot().ruleset_key.clone(),
        );
        if source.binding.key != key
            || source.binding.route.identity.target != *candidate.expected_target()
            || source.binding.route.identity.process_instance_id
                != candidate.current_owner().lease_id.process_instance_id
            || source.binding.route.fencing_token != candidate.source_deployment_fence()
            || source.binding.route.lifecycle != SlotLifecycleV1::Serving
        {
            return Err(ServingSlotRegistryError::V4RouteMismatch);
        }
        let sealed = self.seal_routed_with_intent_v4(source, candidate.intent_id().clone())?;
        let observation = routed_port_observation(&sealed.binding)?;
        Ok((sealed, observation))
    }

    fn recover_routed_claimed(
        &self,
        authorization: &RuntimeSelectedCurrentRoutedClaimedV4,
    ) -> Result<
        (
            Self::RoutedClaimedSealed,
            RuntimeRoutedClaimedSealPortObservationV4,
        ),
        Self::Error,
    > {
        let claimed = self.recover_routed_claimed_state_v4(authorization)?;
        let observation = claimed_port_observation(&claimed)?;
        Ok((claimed, observation))
    }

    fn bind_claim(
        &self,
        source: Self::RoutedSealed,
        receipt: &RuntimeDurableRoutedClaimReceiptV4,
    ) -> Result<
        (
            Self::RoutedClaimedSealed,
            RuntimeRoutedClaimedSealPortObservationV4,
        ),
        Self::Error,
    > {
        validate_routed_claim_receipt(self, &source, receipt)?;
        let claim_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            receipt.terminal_digest().as_bytes(),
        );
        let claimed = self.bind_routed_claim_with_digest_v4(
            source,
            receipt.claim_fence(),
            claim_receipt_digest,
        )?;
        let observation = claimed_port_observation(&claimed)?;
        Ok((claimed, observation))
    }

    fn refence<J: Send, S: Send, C: Send>(
        &self,
        source: Self::RoutedClaimedSealed,
        authorization: &RuntimeAuthorizedRegistryRefenceEvidenceV4<J, S, C>,
    ) -> Result<
        (
            Self::LocallyRefencedSealed,
            RuntimeLocalRefencePortObservationV4,
        ),
        Self::Error,
    > {
        validate_refence_authorization(&source, authorization)?;
        let refenced_at = authorization.minimum_refenced_at();
        if refenced_at >= authorization.owner_expires_at() {
            return Err(ServingSlotRegistryError::V4ReceiptMismatch);
        }
        let local = self.refence_routed_claim_state_v4(
            source,
            authorization.provenance().clone(),
            refenced_at,
        )?;
        let observation = local_port_observation(&local)?;
        Ok((local, observation))
    }

    fn bind_refence(
        &self,
        source: Self::LocallyRefencedSealed,
        receipt: &RuntimeDurableRefenceReceiptV4,
    ) -> Result<
        (
            Self::DurablyRefencedSealed,
            RuntimeDurableRefencePortObservationV4,
        ),
        Self::Error,
    > {
        validate_refence_receipt(&source, receipt)?;
        let local_observation = local_port_observation(&source)?;
        let refence_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            receipt.terminal_digest().as_bytes(),
        );
        let durable = self.bind_durable_refence_with_digest_v4(source, refence_receipt_digest)?;
        Ok((
            durable,
            RuntimeDurableRefencePortObservationV4 {
                local: local_observation,
                refence_receipt_digest: runtime_evidence_digest(
                    *refence_receipt_digest.as_bytes(),
                )?,
            },
        ))
    }

    fn recover_durable_refence(
        &self,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<
        (
            Self::DurablyRefencedSealed,
            RuntimeDurableRefencePortObservationV4,
        ),
        Self::Error,
    > {
        self.recover_durable_refence_state_v4(authorization)
    }

    fn begin_drain(
        &self,
        source: Self::DurablyRefencedSealed,
    ) -> Result<Self::DrainingRefencedSealed, Self::Error> {
        self.begin_drain_refenced_state_v4(source)
    }

    fn remove(
        &self,
        source: Self::DrainingRefencedSealed,
    ) -> Result<(Self::RouteAbsentSealed, RuntimeRouteAbsentPortObservationV4), Self::Error> {
        let observation = self.observe_draining_refenced_v4(&source)?;
        let absent = self.remove_draining_refenced_state_v4(source, observation)?;
        let port_observation = route_absent_port_observation(&absent)?;
        Ok((absent, port_observation))
    }

    fn recover_route_absent(
        &self,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<(Self::RouteAbsentSealed, RuntimeRouteAbsentPortObservationV4), Self::Error> {
        let absent = self.recover_route_absent_state_v4(authorization)?;
        let observation = route_absent_port_observation(&absent)?;
        Ok((absent, observation))
    }

    fn seal_empty_succession(
        &self,
        authorization: &RuntimeSelectedExpiredPreviousOwnerV4,
    ) -> Result<
        (
            Self::EmptySuccessionSealed,
            RuntimeEmptySuccessionPortObservationV4,
        ),
        Self::Error,
    > {
        let request = authorization
            .empty_succession_seal_request()
            .map_err(|_| ServingSlotRegistryError::V4EmptySuccessionMismatch)?;
        let key =
            ServingSlotKeyV1::new(request.slot().guild_id, request.slot().ruleset_key.clone());
        let predecessor_route = PreviousRouteEnvelopeV4::new(
            key.clone(),
            request.predecessor_route().identity.clone(),
            request.predecessor_route().route_incarnation,
            request.predecessor_route().controller_fencing_token,
            request.possible_route_fence_ceiling(),
        )?;
        let successor_identity = RuntimeProcessIdentityV1 {
            target: request.successor_target().clone(),
            runtime_generation: request.predecessor_route().identity.runtime_generation,
            process_instance_id: request.successor_process_instance_id().clone(),
        };
        let cursor = self.recovery_observation_guard_v2()?.into_empty_cursor()?;
        let sealed = self.seal_empty_succession_v4(
            cursor,
            &key,
            request.intent_id().clone(),
            predecessor_route,
            successor_identity,
            request.successor_fence(),
        )?;
        let observation = empty_succession_port_observation(&sealed)?;
        Ok((sealed, observation))
    }

    fn consume_acknowledgement(
        &self,
        source: Self::RouteAbsentSealed,
        receipt: &RuntimeDurableSameProcessDrainAcknowledgementV4,
    ) -> Result<Self::AcknowledgedEmpty, Self::Error> {
        validate_route_absent_receipt(&source, receipt)?;
        let acknowledgement_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            receipt.terminal_digest().as_bytes(),
        );
        self.consume_route_absent_with_digest_v4(source, acknowledgement_receipt_digest)
    }

    fn consume_succession_acknowledgement(
        &self,
        source: Self::EmptySuccessionSealed,
        receipt: &RuntimeDurablePreviousProcessDrainTeardownV4,
    ) -> Result<Self::AcknowledgedEmpty, Self::Error> {
        validate_empty_succession_receipt(&source, receipt)?;
        let acknowledgement_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            receipt.terminal_digest().as_bytes(),
        );
        self.consume_empty_succession_with_digest_v4(source, acknowledgement_receipt_digest)
    }
}

impl RuntimeRoutedDrainRollbackPortV4 for ServingSlotRegistryV1 {
    type Error = ServingSlotRegistryError;
    type RoutedSealed = RoutedSealedV4;
    type Unsealed = RoutedObservedV4;

    fn rollback_routed_seal_v4(
        &self,
        source: Self::RoutedSealed,
        permit: RuntimeRoutedDrainRollbackPermitV4,
    ) -> Result<Self::Unsealed, Self::Error> {
        validate_routed_seal_static!(&source.binding, permit.seal())?;
        self.rollback_routed_seal_state_v4(source)
    }
}

fn validate_routed_claim_receipt(
    registry: &ServingSlotRegistryV1,
    source: &RoutedSealedV4,
    receipt: &RuntimeDurableRoutedClaimReceiptV4,
) -> Result<(), ServingSlotRegistryError> {
    let witness = receipt.source_seal();
    validate_routed_seal_static!(&source.binding, witness)?;
    let state = registry.lock_state()?;
    let (slot_observation, registry_observation_sequence, active_interactions) =
        observe_sealed_route_after_guard_releases(
            &state,
            &source.binding,
            SlotLifecycleV1::Serving,
            true,
        )?;
    if active_interactions != 0
        || witness.admission_generation() != slot_observation.admission_generation
        || witness.slot_observation_sequence() != slot_observation.observation_sequence
        || witness.registry_observation_sequence().get()
            < source.binding.registry_observation_sequence.get()
        || witness.registry_observation_sequence().get() > registry_observation_sequence.get()
    {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    Ok(())
}

fn validate_refence_authorization<J, S, C>(
    source: &RoutedClaimedSealedV4,
    authorization: &RuntimeAuthorizedRegistryRefenceEvidenceV4<J, S, C>,
) -> Result<(), ServingSlotRegistryError> {
    let identity = authorization.finalizer_identity();
    if identity.registry_lifetime_digest().as_bytes() != &source.binding.registry_lifetime_digest
        || identity.process_instance_id()
            != &source.binding.source_route.identity.process_instance_id
        || identity.intent_id() != &source.binding.intent_id
        || identity.intent_id().canonical_bytes().as_slice()
            != source.binding.seal_key.as_bytes().as_slice()
        || identity.seal_generation() != source.binding.seal_generation
        || identity.route_incarnation() != source.binding.source_route.incarnation
        || identity.controller_fence() != source.claim_fence
    {
        return Err(ServingSlotRegistryError::V4CapabilityStale);
    }
    Ok(())
}

fn validate_refence_receipt(
    source: &LocallyRefencedSealedV4,
    receipt: &RuntimeDurableRefenceReceiptV4,
) -> Result<(), ServingSlotRegistryError> {
    validate_routed_seal_static!(&source.binding, receipt.source_seal())?;
    if !route_matches_worker(
        &source.binding.source_route,
        &receipt.source_route().identity,
        receipt.source_route().controller_fencing_token,
        receipt.source_route().route_incarnation,
        SlotLifecycleV1::Serving,
    ) || !route_matches_worker(
        &source.binding.current_route,
        &receipt.removal_target().identity,
        receipt.removal_target().controller_fencing_token,
        receipt.removal_target().route_incarnation,
        SlotLifecycleV1::Serving,
    ) || source.claim_receipt_digest.as_bytes() != receipt.claim_receipt_digest().as_bytes()
        || receipt.registry_observation_sequence()
            != source.binding.registry_observation_sequence.as_non_zero()
    {
        return Err(ServingSlotRegistryError::V4ReceiptMismatch);
    }
    Ok(())
}

fn validate_route_absent_receipt(
    source: &RouteAbsentSealedV4,
    receipt: &RuntimeDurableSameProcessDrainAcknowledgementV4,
) -> Result<(), ServingSlotRegistryError> {
    let witness = receipt.route_absent_witness();
    let key = ServingSlotKeyV1::new(witness.slot().guild_id, witness.slot().ruleset_key.clone());
    if witness.registry_lifetime_digest().as_bytes() != &source.binding.registry_lifetime_digest
        || witness.process_instance_id()
            != &source.binding.source_route.identity.process_instance_id
        || witness.intent_id().canonical_bytes().as_slice() != witness.seal_key().as_slice()
        || source.binding.key != key
        || source.binding.seal_key.as_bytes().as_slice() != witness.seal_key().as_slice()
        || source.binding.seal_generation != witness.seal_generation()
        || source.binding.slot_observation.admission_generation != witness.admission_generation()
        || source.binding.slot_observation.observation_sequence
            != witness.slot_observation_sequence()
        || source.binding.registry_observation_sequence.as_non_zero()
            != witness.registry_observation_sequence()
        || !route_matches_worker(
            &source.binding.source_route,
            &witness.source_route().identity,
            witness.source_route().controller_fencing_token,
            witness.source_route().route_incarnation,
            SlotLifecycleV1::Serving,
        )
        || !route_matches_worker(
            &source.binding.removed_route,
            &witness.removed_route().identity,
            witness.removed_route().controller_fencing_token,
            witness.removed_route().route_incarnation,
            SlotLifecycleV1::Draining,
        )
        || source.claim_receipt_digest.as_bytes() != witness.claim_receipt_digest().as_bytes()
        || source.refence_receipt_digest.as_bytes() != witness.refence_receipt_digest().as_bytes()
    {
        return Err(ServingSlotRegistryError::V4ReceiptMismatch);
    }
    Ok(())
}

fn validate_empty_succession_receipt(
    source: &EmptySuccessionSealedV4,
    receipt: &RuntimeDurablePreviousProcessDrainTeardownV4,
) -> Result<(), ServingSlotRegistryError> {
    let witness = receipt.seal();
    let key = ServingSlotKeyV1::new(witness.slot().guild_id, witness.slot().ruleset_key.clone());
    let predecessor = witness.predecessor_route();
    if witness.registry_lifetime_digest().as_bytes() != &source.registry_lifetime_digest
        || witness.process_instance_id() != &source.successor_identity.process_instance_id
        || witness.intent_id().canonical_bytes().as_slice() != witness.seal_key().as_slice()
        || source.sealed.seal.key != key
        || source.sealed.seal.seal_key.as_bytes().as_slice() != witness.seal_key().as_slice()
        || source.sealed.seal.seal_generation != witness.seal_generation()
        || source.sealed.slot_observation.admission_generation != witness.admission_generation()
        || source.sealed.slot_observation.observation_sequence
            != witness.slot_observation_sequence()
        || source
            .sealed
            .registry_observation
            .observation_sequence()
            .as_non_zero()
            != witness.registry_observation_sequence()
        || source.predecessor_route.identity != predecessor.identity
        || source.predecessor_route.incarnation != predecessor.route_incarnation
        || source.predecessor_route.source_route_fence != predecessor.controller_fencing_token
        || source.predecessor_route.possible_route_fence_ceiling
            != witness.possible_route_fence_ceiling()
        || source.successor_identity != *witness.successor_identity()
        || source.successor_fence != witness.successor_fence()
    {
        return Err(ServingSlotRegistryError::V4ReceiptMismatch);
    }
    Ok(())
}
