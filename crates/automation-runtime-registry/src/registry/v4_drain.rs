use std::num::NonZeroU64;
use std::sync::{Arc, Weak};

use automation_runtime_controller::{
    RuntimeDrainIntentIdV2, RuntimeExactLocalRouteIdentityV2, RuntimeRouteMutationProvenanceV2,
    RuntimeServingSlotV2,
};
use automation_runtime_convergence::{FencingToken, RuntimeProcessIdentityV1};
use automation_runtime_worker::{
    RuntimeDurableRefencePortObservationV4, RuntimeEmptySuccessionPortObservationV4,
    RuntimeLocalRefencePortObservationV4, RuntimePendingDrainEvidenceDigestV4,
    RuntimeRouteAbsentPortObservationV4, RuntimeRoutedClaimedSealPortObservationV4,
    RuntimeRoutedSealPortObservationV4, RuntimeSelectedCurrentRefencedV4,
    RuntimeSelectedCurrentRoutedClaimedV4,
};
use chrono::{DateTime, Utc};

use super::{
    advance_slot_mutation, advance_slot_seal, atomic_observation_v2, ensure_high_water,
    ensure_slot_open, ensure_slot_unsealed, registry_recovery_observation_v2, route_witness_v1,
    selected_route, validate_new_fence, FenceHighWater, RegistryInner, RegistryState, RouteRecord,
    SealedEmptyRecoveryDrainClaimV2, ServingSlotRegistryV1, SlotDrainClaimSealV2, SlotLifecycleV1,
    SlotMutationTokenV1, SlotRouteWitnessV1, SlotSealState,
};
use crate::{
    RegistryEmptyRecoveryCursorV2, RegistryGlobalObservationSequenceV2, ServingSlotKeyV1,
    ServingSlotRegistryError, SlotAdmissionStateV2, SlotAtomicObservationV2, SlotSealKeyV2,
};

macro_rules! validate_routed_seal_static {
    ($binding:expr, $witness:expr) => {{
        let binding = $binding;
        let witness = $witness;
        let key =
            ServingSlotKeyV1::new(witness.slot().guild_id, witness.slot().ruleset_key.clone());
        if witness.registry_lifetime_digest().as_bytes() != &binding.registry_lifetime_digest
            || witness.process_instance_id() != &binding.source_route.identity.process_instance_id
            || witness.intent_id() != &binding.intent_id
            || witness.intent_id().canonical_bytes().as_slice() != witness.seal_key().as_slice()
            || binding.key != key
            || binding.seal_key.as_bytes().as_slice() != witness.seal_key().as_slice()
            || binding.seal_generation != witness.seal_generation()
            || binding.routed_slot_observation.admission_generation
                != witness.admission_generation()
            || binding.routed_slot_observation.observation_sequence
                != witness.slot_observation_sequence()
            || binding.routed_registry_observation_sequence.as_non_zero()
                != witness.registry_observation_sequence()
            || !route_matches_worker(
                &binding.source_route,
                &witness.route().identity,
                witness.route().controller_fencing_token,
                witness.route().route_incarnation,
                SlotLifecycleV1::Serving,
            )
        {
            Err(ServingSlotRegistryError::V4ReceiptMismatch)
        } else {
            Ok(())
        }
    }};
}

mod state;
mod worker_port;

use state::{AbsentRouteBindingV4, RoutedObservationBindingV4, SealedRouteBindingV4};
pub use state::{
    AcknowledgedEmptyV4, DrainingRefencedObservationV4, DrainingRefencedSealedV4,
    DurablyRefencedSealedV4, EmptySuccessionSealedV4, LocallyRefencedSealedV4,
    PreviousRouteEnvelopeV4, RegistryDurableReceiptDigestV4, RouteAbsentSealedV4,
    RoutedClaimedSealedV4, RoutedObservedV4, RoutedSealedObservationV4, RoutedSealedV4,
};

struct LocalRefenceRecoveryEvidenceV4 {
    claim_receipt_digest: RegistryDurableReceiptDigestV4,
    provenance: RuntimeRouteMutationProvenanceV2,
    refenced_at: DateTime<Utc>,
    registry_observation_sequence: NonZeroU64,
}

impl ServingSlotRegistryV1 {
    pub fn observe_routed_v4(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<RoutedObservedV4, ServingSlotRegistryError> {
        self.ensure_registry_token(token)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(token.key())
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
        ensure_single_current_route(slot)?;
        ensure_high_water(Some(slot), token)?;
        let current = slot
            .current
            .as_ref()
            .filter(|route| route.matches(token))
            .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
        if current.lifecycle != SlotLifecycleV1::Serving {
            return Err(ServingSlotRegistryError::V4LifecycleMismatch);
        }
        let total_active_interactions = total_active_interactions(slot)?;
        Ok(RoutedObservedV4 {
            binding: RoutedObservationBindingV4 {
                registry: Arc::downgrade(&self.inner),
                key: token.key().clone(),
                route: route_witness_v1(current),
                slot_observation: atomic_observation_v2(slot)?,
                registry_observation_sequence: registry_observation_sequence(&state)?,
                total_active_interactions,
            },
        })
    }

    fn seal_routed_with_intent_v4(
        &self,
        source: RoutedObservedV4,
        intent_id: RuntimeDrainIntentIdV2,
    ) -> Result<RoutedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        if source.binding.total_active_interactions != 0 {
            return Err(ServingSlotRegistryError::V4GuardMismatch);
        }
        let seal_key = SlotSealKeyV2::try_from(intent_id.canonical_bytes().as_slice())
            .map_err(|_| ServingSlotRegistryError::V4CapabilityStale)?;
        let mut state = self.lock_state()?;
        validate_routed_observation(&state, &source.binding)?;
        let (seal_generation, slot_observation, route) = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.binding.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            let route = source.binding.route.clone();
            let seal_generation = advance_slot_seal(observation, slot, false)?;
            slot.seal = Some(SlotSealState {
                seal_key,
                seal_generation,
                route: Some(route.clone()),
                source_route: Some(route.clone()),
                source_admission_generation: slot.admission_generation,
                source_observation_sequence: slot.observation_sequence,
                source_registry_observation_sequence: observation.sequence,
            });
            (seal_generation, atomic_observation_v2(slot)?, route)
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        let routed_slot_observation = slot_observation.clone();
        Ok(RoutedSealedV4 {
            binding: SealedRouteBindingV4 {
                registry: Arc::downgrade(&self.inner),
                registry_lifetime_digest: self.inner.lifetime_digest,
                intent_id,
                key: source.binding.key,
                seal_key,
                seal_generation,
                source_route: route.clone(),
                current_route: route,
                routed_slot_observation,
                routed_registry_observation_sequence: registry_observation_sequence,
                slot_observation,
                registry_observation_sequence,
                total_active_interactions: source.binding.total_active_interactions,
            },
        })
    }

    pub fn observe_routed_sealed_v4(
        &self,
        source: &RoutedSealedV4,
    ) -> Result<RoutedSealedObservationV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let state = self.lock_state()?;
        let (slot_observation, registry_observation_sequence, total_active_interactions) =
            observe_sealed_route_after_guard_releases(
                &state,
                &source.binding,
                SlotLifecycleV1::Serving,
                false,
            )?;
        Ok(RoutedSealedObservationV4 {
            registry: Arc::downgrade(&self.inner),
            key: source.binding.key.clone(),
            seal_key: source.binding.seal_key,
            seal_generation: source.binding.seal_generation,
            slot_observation,
            registry_observation_sequence,
            total_active_interactions,
        })
    }

    fn bind_routed_claim_with_digest_v4(
        &self,
        source: RoutedSealedV4,
        claim_fence: FencingToken,
        claim_receipt_digest: RegistryDurableReceiptDigestV4,
    ) -> Result<RoutedClaimedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let expected = source
            .binding
            .source_route
            .fencing_token
            .next()
            .map_err(|_| ServingSlotRegistryError::FencingTokenExhausted)?;
        if claim_fence != expected {
            return Err(ServingSlotRegistryError::V4FenceMismatch);
        }
        let state = self.lock_state()?;
        let (slot_observation, registry_observation_sequence, total_active_interactions) =
            observe_sealed_route_after_guard_releases(
                &state,
                &source.binding,
                SlotLifecycleV1::Serving,
                true,
            )?;
        let binding = SealedRouteBindingV4 {
            registry: source.binding.registry,
            registry_lifetime_digest: source.binding.registry_lifetime_digest,
            intent_id: source.binding.intent_id,
            key: source.binding.key,
            seal_key: source.binding.seal_key,
            seal_generation: source.binding.seal_generation,
            source_route: source.binding.source_route,
            current_route: source.binding.current_route,
            routed_slot_observation: source.binding.routed_slot_observation,
            routed_registry_observation_sequence: source
                .binding
                .routed_registry_observation_sequence,
            slot_observation,
            registry_observation_sequence,
            total_active_interactions,
        };
        Ok(RoutedClaimedSealedV4 {
            binding,
            claim_fence,
            claim_receipt_digest,
        })
    }

    fn rollback_routed_seal_state_v4(
        &self,
        source: RoutedSealedV4,
    ) -> Result<RoutedObservedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let mut state = self.lock_state()?;
        observe_sealed_route_after_guard_releases(
            &state,
            &source.binding,
            SlotLifecycleV1::Serving,
            false,
        )?;
        let (route, slot_observation, total_active_interactions) = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.binding.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            advance_slot_mutation(observation, slot)?;
            slot.seal = None;
            let current = slot
                .current
                .as_ref()
                .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
            (
                route_witness_v1(current),
                atomic_observation_v2(slot)?,
                total_active_interactions(slot)?,
            )
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        Ok(RoutedObservedV4 {
            binding: RoutedObservationBindingV4 {
                registry: source.binding.registry,
                key: source.binding.key,
                route,
                slot_observation,
                registry_observation_sequence,
                total_active_interactions,
            },
        })
    }

    fn refence_routed_claim_state_v4(
        &self,
        source: RoutedClaimedSealedV4,
        provenance: RuntimeRouteMutationProvenanceV2,
        refenced_at: DateTime<Utc>,
    ) -> Result<LocallyRefencedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let mut state = self.lock_state()?;
        validate_sealed_route(&state, &source.binding, SlotLifecycleV1::Serving)?;
        let slot_observation = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.binding.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            advance_slot_mutation(observation, slot)?;
            let current = slot
                .current
                .as_mut()
                .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
            current.fencing_token = source.claim_fence;
            let current_route = route_witness_v1(current);
            let high_water = slot
                .high_water
                .as_mut()
                .ok_or(ServingSlotRegistryError::V4FenceMismatch)?;
            high_water.fencing_token = source.claim_fence;
            let seal = slot
                .seal
                .as_mut()
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            seal.route = Some(current_route);
            atomic_observation_v2(slot)?
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        let slot = state
            .slots
            .get(&source.binding.key)
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        let current_route = route_witness_v1(
            slot.current
                .as_ref()
                .ok_or(ServingSlotRegistryError::V4RouteMismatch)?,
        );
        Ok(LocallyRefencedSealedV4 {
            binding: SealedRouteBindingV4 {
                registry: source.binding.registry,
                registry_lifetime_digest: source.binding.registry_lifetime_digest,
                intent_id: source.binding.intent_id,
                key: source.binding.key,
                seal_key: source.binding.seal_key,
                seal_generation: source.binding.seal_generation,
                source_route: source.binding.source_route,
                current_route,
                routed_slot_observation: source.binding.routed_slot_observation,
                routed_registry_observation_sequence: source
                    .binding
                    .routed_registry_observation_sequence,
                slot_observation,
                registry_observation_sequence,
                total_active_interactions: 0,
            },
            claim_receipt_digest: source.claim_receipt_digest,
            provenance,
            refenced_at,
            refenced_registry_observation_sequence: registry_observation_sequence,
        })
    }

    fn bind_durable_refence_with_digest_v4(
        &self,
        source: LocallyRefencedSealedV4,
        refence_receipt_digest: RegistryDurableReceiptDigestV4,
    ) -> Result<DurablyRefencedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let state = self.lock_state()?;
        validate_sealed_route(&state, &source.binding, SlotLifecycleV1::Serving)?;
        Ok(DurablyRefencedSealedV4 {
            binding: source.binding,
            claim_receipt_digest: source.claim_receipt_digest,
            refence_receipt_digest,
        })
    }

    fn begin_drain_refenced_state_v4(
        &self,
        source: DurablyRefencedSealedV4,
    ) -> Result<DrainingRefencedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let mut state = self.lock_state()?;
        validate_sealed_route(&state, &source.binding, SlotLifecycleV1::Serving)?;
        let slot_observation = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.binding.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            advance_slot_mutation(observation, slot)?;
            let current = slot
                .current
                .as_mut()
                .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
            current.lifecycle = SlotLifecycleV1::Draining;
            let current_route = route_witness_v1(current);
            let seal = slot
                .seal
                .as_mut()
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            seal.route = Some(current_route);
            atomic_observation_v2(slot)?
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        let slot = state
            .slots
            .get(&source.binding.key)
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        let current_route = route_witness_v1(
            slot.current
                .as_ref()
                .ok_or(ServingSlotRegistryError::V4RouteMismatch)?,
        );
        Ok(DrainingRefencedSealedV4 {
            binding: SealedRouteBindingV4 {
                registry: source.binding.registry,
                registry_lifetime_digest: source.binding.registry_lifetime_digest,
                intent_id: source.binding.intent_id,
                key: source.binding.key,
                seal_key: source.binding.seal_key,
                seal_generation: source.binding.seal_generation,
                source_route: source.binding.source_route,
                current_route,
                routed_slot_observation: source.binding.routed_slot_observation,
                routed_registry_observation_sequence: source
                    .binding
                    .routed_registry_observation_sequence,
                slot_observation,
                registry_observation_sequence,
                total_active_interactions: 0,
            },
            claim_receipt_digest: source.claim_receipt_digest,
            refence_receipt_digest: source.refence_receipt_digest,
        })
    }

    pub fn observe_draining_refenced_v4(
        &self,
        source: &DrainingRefencedSealedV4,
    ) -> Result<DrainingRefencedObservationV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let state = self.lock_state()?;
        validate_sealed_route(&state, &source.binding, SlotLifecycleV1::Draining)?;
        let slot = state
            .slots
            .get(&source.binding.key)
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        Ok(DrainingRefencedObservationV4 {
            registry: Arc::downgrade(&self.inner),
            key: source.binding.key.clone(),
            seal_key: source.binding.seal_key,
            seal_generation: source.binding.seal_generation,
            slot_observation: atomic_observation_v2(slot)?,
            registry_observation_sequence: registry_observation_sequence(&state)?,
            total_active_interactions: total_active_interactions(slot)?,
        })
    }

    fn remove_draining_refenced_state_v4(
        &self,
        source: DrainingRefencedSealedV4,
        observation: DrainingRefencedObservationV4,
    ) -> Result<RouteAbsentSealedV4, ServingSlotRegistryError> {
        validate_draining_observation_binding(self, &source, &observation)?;
        if observation.total_active_interactions != 0 {
            return Err(ServingSlotRegistryError::V4GuardMismatch);
        }
        let mut state = self.lock_state()?;
        validate_sealed_route(&state, &source.binding, SlotLifecycleV1::Draining)?;
        if observation.slot_observation != source.binding.slot_observation
            || observation.registry_observation_sequence
                < source.binding.registry_observation_sequence
        {
            return Err(ServingSlotRegistryError::V4ObservationMismatch);
        }
        let slot_observation = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.binding.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            advance_slot_mutation(observation, slot)?;
            slot.current = None;
            let seal = slot
                .seal
                .as_mut()
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            seal.route = None;
            atomic_observation_v2(slot)?
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        Ok(RouteAbsentSealedV4 {
            binding: AbsentRouteBindingV4 {
                registry: source.binding.registry,
                registry_lifetime_digest: source.binding.registry_lifetime_digest,
                intent_id: source.binding.intent_id,
                key: source.binding.key,
                seal_key: source.binding.seal_key,
                seal_generation: source.binding.seal_generation,
                source_route: source.binding.source_route,
                removed_route: source.binding.current_route,
                slot_observation,
                registry_observation_sequence,
            },
            claim_receipt_digest: source.claim_receipt_digest,
            refence_receipt_digest: source.refence_receipt_digest,
        })
    }

    fn consume_route_absent_with_digest_v4(
        &self,
        source: RouteAbsentSealedV4,
        acknowledgement_receipt_digest: RegistryDurableReceiptDigestV4,
    ) -> Result<AcknowledgedEmptyV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let mut state = self.lock_state()?;
        validate_absent_route(&state, &source.binding)?;
        let slot_observation = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.binding.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            advance_slot_mutation(observation, slot)?;
            slot.seal = None;
            atomic_observation_v2(slot)?
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        Ok(AcknowledgedEmptyV4 {
            registry: source.binding.registry,
            key: source.binding.key,
            slot_observation,
            registry_observation_sequence,
            claim_receipt_digest: Some(source.claim_receipt_digest),
            refence_receipt_digest: Some(source.refence_receipt_digest),
            acknowledgement_receipt_digest,
            successor_fence: source.binding.removed_route.fencing_token,
        })
    }

    pub fn seal_empty_succession_v4(
        &self,
        cursor: RegistryEmptyRecoveryCursorV2,
        key: &ServingSlotKeyV1,
        intent_id: RuntimeDrainIntentIdV2,
        predecessor_route: PreviousRouteEnvelopeV4,
        successor_identity: RuntimeProcessIdentityV1,
        successor_fence: FencingToken,
    ) -> Result<EmptySuccessionSealedV4, ServingSlotRegistryError> {
        let seal_key = SlotSealKeyV2::try_from(intent_id.canonical_bytes().as_slice())
            .map_err(|_| ServingSlotRegistryError::V4EmptySuccessionMismatch)?;
        if predecessor_route.key() != key
            || !key.matches_target(&successor_identity.target)
            || predecessor_route.identity().target != successor_identity.target
            || predecessor_route.identity().runtime_generation
                != successor_identity.runtime_generation
            || predecessor_route.identity().process_instance_id
                == successor_identity.process_instance_id
            || predecessor_route.possible_route_fence_ceiling().next().ok() != Some(successor_fence)
        {
            return Err(ServingSlotRegistryError::V4EmptySuccessionMismatch);
        }
        let sealed = self.seal_empty_recovery_drain_claim_v2(cursor, key, seal_key)?;
        Ok(EmptySuccessionSealedV4 {
            sealed,
            registry_lifetime_digest: self.inner.lifetime_digest,
            intent_id,
            predecessor_route,
            successor_identity,
            successor_fence,
        })
    }

    fn consume_empty_succession_with_digest_v4(
        &self,
        source: EmptySuccessionSealedV4,
        acknowledgement_receipt_digest: RegistryDurableReceiptDigestV4,
    ) -> Result<AcknowledgedEmptyV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.sealed.seal.registry)?;
        let mut state = self.lock_state()?;
        if registry_recovery_observation_v2(&state)? != source.sealed.registry_observation {
            return Err(ServingSlotRegistryError::V4ObservationMismatch);
        }
        let slot_observation = {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&source.sealed.seal.key)
                .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
            ensure_slot_open(slot)?;
            if atomic_observation_v2(slot)? != source.sealed.slot_observation
                || selected_route(slot).is_some()
                || total_active_interactions(slot)? != 0
            {
                return Err(ServingSlotRegistryError::V4ObservationMismatch);
            }
            let seal_matches = slot.seal.as_ref().is_some_and(|seal| {
                seal.seal_key == source.sealed.seal.seal_key
                    && seal.seal_generation == source.sealed.seal.seal_generation
                    && seal.route.is_none()
            });
            if !seal_matches {
                return Err(ServingSlotRegistryError::V4CapabilityStale);
            }
            validate_new_fence(
                Some(slot),
                &source.successor_identity,
                source.successor_fence,
            )?;
            advance_slot_mutation(observation, slot)?;
            slot.high_water = Some(FenceHighWater {
                generation: source.successor_identity.runtime_generation,
                fencing_token: source.successor_fence,
                identity: source.successor_identity.clone(),
            });
            slot.seal = None;
            atomic_observation_v2(slot)?
        };
        let registry_observation_sequence = registry_observation_sequence(&state)?;
        Ok(AcknowledgedEmptyV4 {
            registry: source.sealed.seal.registry,
            key: source.sealed.seal.key,
            slot_observation,
            registry_observation_sequence,
            claim_receipt_digest: None,
            refence_receipt_digest: None,
            acknowledgement_receipt_digest,
            successor_fence: source.successor_fence,
        })
    }

    fn recover_routed_claimed_state_v4(
        &self,
        authorization: &RuntimeSelectedCurrentRoutedClaimedV4,
    ) -> Result<RoutedClaimedSealedV4, ServingSlotRegistryError> {
        let candidate = authorization.candidate();
        let source_route = worker_route_witness(candidate.source_route(), SlotLifecycleV1::Serving);
        let claim_fence = candidate.claim().controller_fencing_token();
        if source_route.fencing_token.next().ok() != Some(claim_fence) {
            return Err(ServingSlotRegistryError::V4FenceMismatch);
        }
        let binding = self.recover_sealed_route_binding_v4(
            candidate.intent_id().clone(),
            source_route,
            &[
                candidate.source_route().controller_fencing_token,
                claim_fence,
            ],
            SlotLifecycleV1::Serving,
            candidate.claim().progress().seal().seal_generation(),
            candidate
                .claim()
                .progress()
                .seal()
                .registry_observation_sequence(),
        )?;
        Ok(RoutedClaimedSealedV4 {
            binding,
            claim_fence,
            claim_receipt_digest: RegistryDurableReceiptDigestV4::from_checked_bytes(
                candidate.claim_terminal_digest().as_bytes(),
            ),
        })
    }

    fn recover_durable_refence_state_v4(
        &self,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<
        (
            DurablyRefencedSealedV4,
            RuntimeDurableRefencePortObservationV4,
        ),
        ServingSlotRegistryError,
    > {
        let candidate = authorization.candidate();
        let source_route = worker_route_witness(candidate.source_route(), SlotLifecycleV1::Serving);
        let binding = self.recover_sealed_route_binding_v4(
            candidate.intent_id().clone(),
            source_route,
            &[candidate.removal_target().controller_fencing_token],
            SlotLifecycleV1::Serving,
            candidate.claim().progress().seal().seal_generation(),
            candidate
                .claim()
                .progress()
                .seal()
                .registry_observation_sequence(),
        )?;
        let claim_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.claim_terminal_digest().as_bytes(),
        );
        let local = LocallyRefencedSealedV4 {
            provenance: candidate
                .claim()
                .progress()
                .provenance()
                .ok_or(ServingSlotRegistryError::V4ReceiptMismatch)?
                .clone(),
            refenced_at: candidate
                .claim()
                .progress()
                .refenced_at()
                .ok_or(ServingSlotRegistryError::V4ReceiptMismatch)?,
            refenced_registry_observation_sequence: RegistryGlobalObservationSequenceV2::new(
                candidate
                    .claim()
                    .progress()
                    .registry_observation_sequence()
                    .ok_or(ServingSlotRegistryError::V4ObservationMismatch)?,
            ),
            binding,
            claim_receipt_digest,
        };
        let local_observation = local_port_observation(&local)?;
        let refence_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.refence_terminal_digest().as_bytes(),
        );
        let durable = DurablyRefencedSealedV4 {
            binding: local.binding,
            claim_receipt_digest,
            refence_receipt_digest,
        };
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

    fn recover_route_absent_state_v4(
        &self,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<RouteAbsentSealedV4, ServingSlotRegistryError> {
        let candidate = authorization.candidate();
        let source_route = worker_route_witness(candidate.source_route(), SlotLifecycleV1::Serving);
        let removed_route =
            worker_route_witness(candidate.removal_target(), SlotLifecycleV1::Draining);
        let key = ServingSlotKeyV1::new(
            candidate.slot().guild_id,
            candidate.slot().ruleset_key.clone(),
        );
        let seal_key = SlotSealKeyV2::try_from(candidate.intent_id().canonical_bytes().as_slice())
            .map_err(|_| ServingSlotRegistryError::V4CapabilityStale)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(&key)
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        ensure_slot_open(slot)?;
        if selected_route(slot).is_some()
            || !slot.retired.is_empty()
            || total_active_interactions(slot)? != 0
        {
            return Err(ServingSlotRegistryError::V4RouteMismatch);
        }
        let seal = slot
            .seal
            .as_ref()
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        if seal.seal_key != seal_key
            || seal.seal_generation != candidate.claim().progress().seal().seal_generation()
            || seal.source_route.as_ref() != Some(&source_route)
            || seal.route.is_some()
            || seal.source_registry_observation_sequence.as_non_zero()
                != candidate
                    .claim()
                    .progress()
                    .seal()
                    .registry_observation_sequence()
        {
            return Err(ServingSlotRegistryError::V4ObservationMismatch);
        }
        ensure_high_water_matches_witness(slot, &removed_route)?;
        let binding = AbsentRouteBindingV4 {
            registry: Arc::downgrade(&self.inner),
            registry_lifetime_digest: self.inner.lifetime_digest,
            intent_id: candidate.intent_id().clone(),
            key,
            seal_key,
            seal_generation: seal.seal_generation,
            source_route,
            removed_route,
            slot_observation: atomic_observation_v2(slot)?,
            registry_observation_sequence: registry_observation_sequence(&state)?,
        };
        validate_absent_route(&state, &binding)?;
        Ok(RouteAbsentSealedV4 {
            binding,
            claim_receipt_digest: RegistryDurableReceiptDigestV4::from_checked_bytes(
                candidate.claim_terminal_digest().as_bytes(),
            ),
            refence_receipt_digest: RegistryDurableReceiptDigestV4::from_checked_bytes(
                candidate.refence_terminal_digest().as_bytes(),
            ),
        })
    }

    fn recover_sealed_route_binding_v4(
        &self,
        intent_id: RuntimeDrainIntentIdV2,
        source_route: SlotRouteWitnessV1,
        allowed_current_fences: &[FencingToken],
        lifecycle: SlotLifecycleV1,
        seal_generation: NonZeroU64,
        seal_registry_observation_sequence: NonZeroU64,
    ) -> Result<SealedRouteBindingV4, ServingSlotRegistryError> {
        let key = ServingSlotKeyV1::new(
            source_route.identity.target.guild_id,
            source_route.identity.target.ruleset_key.clone(),
        );
        let seal_key = SlotSealKeyV2::try_from(intent_id.canonical_bytes().as_slice())
            .map_err(|_| ServingSlotRegistryError::V4CapabilityStale)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(&key)
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        ensure_slot_open(slot)?;
        ensure_single_current_route(slot)?;
        if total_active_interactions(slot)? != 0 {
            return Err(ServingSlotRegistryError::V4GuardMismatch);
        }
        let current_route = route_witness_v1(
            slot.current
                .as_ref()
                .ok_or(ServingSlotRegistryError::V4RouteMismatch)?,
        );
        if current_route.identity != source_route.identity
            || current_route.incarnation != source_route.incarnation
            || current_route.lifecycle != lifecycle
            || !allowed_current_fences.contains(&current_route.fencing_token)
        {
            return Err(ServingSlotRegistryError::V4RouteMismatch);
        }
        let seal = slot
            .seal
            .as_ref()
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        if seal.seal_key != seal_key
            || seal.seal_generation != seal_generation
            || seal.source_route.as_ref() != Some(&source_route)
            || seal.route.as_ref() != Some(&current_route)
            || seal.source_registry_observation_sequence.as_non_zero()
                != seal_registry_observation_sequence
        {
            return Err(ServingSlotRegistryError::V4ObservationMismatch);
        }
        ensure_high_water_matches_witness(slot, &current_route)?;
        let routed_slot_observation = SlotAtomicObservationV2 {
            route: Some(source_route.clone()),
            admission_state: SlotAdmissionStateV2::DrainClaimSealed {
                seal_key,
                seal_generation,
            },
            active_interactions: 0,
            admission_generation: seal.source_admission_generation,
            observation_sequence: seal.source_observation_sequence,
        };
        Ok(SealedRouteBindingV4 {
            registry: Arc::downgrade(&self.inner),
            registry_lifetime_digest: self.inner.lifetime_digest,
            intent_id,
            key,
            seal_key,
            seal_generation,
            source_route,
            current_route,
            routed_slot_observation,
            routed_registry_observation_sequence: seal.source_registry_observation_sequence,
            slot_observation: atomic_observation_v2(slot)?,
            registry_observation_sequence: registry_observation_sequence(&state)?,
            total_active_interactions: 0,
        })
    }

    pub fn checkpoint_locally_refenced_seal_v4(
        &self,
        source: LocallyRefencedSealedV4,
    ) -> Result<SlotDrainClaimSealV2, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let state = self.lock_state()?;
        validate_sealed_route(&state, &source.binding, SlotLifecycleV1::Serving)?;
        Ok(recovery_seal_from_binding(source.binding))
    }

    pub fn resume_locally_refenced_sealed_v4(
        &self,
        seal: SlotDrainClaimSealV2,
        authorization: &RuntimeSelectedCurrentRoutedClaimedV4,
    ) -> Result<LocallyRefencedSealedV4, ServingSlotRegistryError> {
        let candidate = authorization.candidate();
        validate_recovery_authorization(
            &seal,
            candidate.intent_id().canonical_bytes(),
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            candidate.source_route().identity.clone(),
            candidate.source_route().controller_fencing_token,
            candidate.source_route().route_incarnation,
        )?;
        let removal_target = SlotRouteWitnessV1 {
            identity: candidate.source_route().identity.clone(),
            fencing_token: candidate.claim().controller_fencing_token(),
            incarnation: candidate.source_route().route_incarnation,
            lifecycle: SlotLifecycleV1::Serving,
        };
        let claim_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.claim_terminal_digest().as_bytes(),
        );
        self.resume_locally_refenced_with_evidence_v4(
            seal,
            candidate.intent_id().clone(),
            removal_target,
            LocalRefenceRecoveryEvidenceV4 {
                claim_receipt_digest,
                provenance: candidate
                    .claim()
                    .progress()
                    .provenance()
                    .ok_or(ServingSlotRegistryError::V4ReceiptMismatch)?
                    .clone(),
                refenced_at: candidate
                    .claim()
                    .progress()
                    .refenced_at()
                    .ok_or(ServingSlotRegistryError::V4ReceiptMismatch)?,
                registry_observation_sequence: candidate
                    .claim()
                    .progress()
                    .registry_observation_sequence()
                    .ok_or(ServingSlotRegistryError::V4ObservationMismatch)?,
            },
        )
    }

    fn resume_locally_refenced_with_evidence_v4(
        &self,
        seal: SlotDrainClaimSealV2,
        intent_id: RuntimeDrainIntentIdV2,
        removal_target: SlotRouteWitnessV1,
        evidence: LocalRefenceRecoveryEvidenceV4,
    ) -> Result<LocallyRefencedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &seal.registry)?;
        let source_route = seal
            .route
            .clone()
            .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
        validate_refence_pair(&source_route, &removal_target)?;
        let state = self.lock_state()?;
        let binding = reconstruct_sealed_route_binding(
            self,
            &state,
            &seal,
            intent_id.clone(),
            source_route,
            removal_target,
        )?;
        validate_sealed_route(&state, &binding, SlotLifecycleV1::Serving)?;
        Ok(LocallyRefencedSealedV4 {
            binding,
            claim_receipt_digest: evidence.claim_receipt_digest,
            provenance: evidence.provenance,
            refenced_at: evidence.refenced_at,
            refenced_registry_observation_sequence: RegistryGlobalObservationSequenceV2::new(
                evidence.registry_observation_sequence,
            ),
        })
    }

    pub fn resume_durably_refenced_sealed_v4(
        &self,
        seal: SlotDrainClaimSealV2,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<DurablyRefencedSealedV4, ServingSlotRegistryError> {
        let candidate = authorization.candidate();
        validate_recovery_authorization(
            &seal,
            candidate.intent_id().canonical_bytes(),
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            candidate.source_route().identity.clone(),
            candidate.source_route().controller_fencing_token,
            candidate.source_route().route_incarnation,
        )?;
        let removal_target = SlotRouteWitnessV1 {
            identity: candidate.removal_target().identity.clone(),
            fencing_token: candidate.removal_target().controller_fencing_token,
            incarnation: candidate.removal_target().route_incarnation,
            lifecycle: SlotLifecycleV1::Serving,
        };
        let claim_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.claim_terminal_digest().as_bytes(),
        );
        let refence_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.refence_terminal_digest().as_bytes(),
        );
        self.resume_durably_refenced_with_evidence_v4(
            seal,
            candidate.intent_id().clone(),
            removal_target,
            claim_receipt_digest,
            refence_receipt_digest,
        )
    }

    fn resume_durably_refenced_with_evidence_v4(
        &self,
        seal: SlotDrainClaimSealV2,
        intent_id: RuntimeDrainIntentIdV2,
        removal_target: SlotRouteWitnessV1,
        claim_receipt_digest: RegistryDurableReceiptDigestV4,
        refence_receipt_digest: RegistryDurableReceiptDigestV4,
    ) -> Result<DurablyRefencedSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &seal.registry)?;
        let source_route = seal
            .route
            .clone()
            .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
        validate_refence_pair(&source_route, &removal_target)?;
        let state = self.lock_state()?;
        let binding = reconstruct_sealed_route_binding(
            self,
            &state,
            &seal,
            intent_id,
            source_route,
            removal_target,
        )?;
        validate_sealed_route(&state, &binding, SlotLifecycleV1::Serving)?;
        Ok(DurablyRefencedSealedV4 {
            binding,
            claim_receipt_digest,
            refence_receipt_digest,
        })
    }

    pub fn checkpoint_route_absent_seal_v4(
        &self,
        source: RouteAbsentSealedV4,
    ) -> Result<SlotDrainClaimSealV2, ServingSlotRegistryError> {
        ensure_registry_binding(self, &source.binding.registry)?;
        let state = self.lock_state()?;
        validate_absent_route(&state, &source.binding)?;
        Ok(recovery_seal_from_absent_binding(source.binding))
    }

    pub fn resume_route_absent_sealed_v4(
        &self,
        seal: SlotDrainClaimSealV2,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<RouteAbsentSealedV4, ServingSlotRegistryError> {
        let candidate = authorization.candidate();
        validate_recovery_authorization(
            &seal,
            candidate.intent_id().canonical_bytes(),
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            candidate.source_route().identity.clone(),
            candidate.source_route().controller_fencing_token,
            candidate.source_route().route_incarnation,
        )?;
        let removal_target = SlotRouteWitnessV1 {
            identity: candidate.removal_target().identity.clone(),
            fencing_token: candidate.removal_target().controller_fencing_token,
            incarnation: candidate.removal_target().route_incarnation,
            lifecycle: SlotLifecycleV1::Draining,
        };
        let claim_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.claim_terminal_digest().as_bytes(),
        );
        let refence_receipt_digest = RegistryDurableReceiptDigestV4::from_checked_bytes(
            candidate.refence_terminal_digest().as_bytes(),
        );
        self.resume_route_absent_with_evidence_v4(
            seal,
            candidate.intent_id().clone(),
            removal_target,
            claim_receipt_digest,
            refence_receipt_digest,
        )
    }

    fn resume_route_absent_with_evidence_v4(
        &self,
        seal: SlotDrainClaimSealV2,
        intent_id: RuntimeDrainIntentIdV2,
        removal_target: SlotRouteWitnessV1,
        claim_receipt_digest: RegistryDurableReceiptDigestV4,
        refence_receipt_digest: RegistryDurableReceiptDigestV4,
    ) -> Result<RouteAbsentSealedV4, ServingSlotRegistryError> {
        ensure_registry_binding(self, &seal.registry)?;
        let source_route = seal
            .route
            .clone()
            .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
        validate_absent_refence_pair(&source_route, &removal_target)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(&seal.key)
            .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
        ensure_slot_open(slot)?;
        if selected_route(slot).is_some() || total_active_interactions(slot)? != 0 {
            return Err(ServingSlotRegistryError::V4RouteMismatch);
        }
        let seal_matches = slot.seal.as_ref().is_some_and(|current| {
            current.seal_key == seal.seal_key
                && current.seal_generation == seal.seal_generation
                && current.route.is_none()
        });
        if !seal_matches {
            return Err(ServingSlotRegistryError::V4CapabilityStale);
        }
        ensure_high_water_matches_witness(slot, &removal_target)?;
        let binding = AbsentRouteBindingV4 {
            registry: Arc::downgrade(&self.inner),
            registry_lifetime_digest: self.inner.lifetime_digest,
            intent_id,
            key: seal.key,
            seal_key: seal.seal_key,
            seal_generation: seal.seal_generation,
            source_route,
            removed_route: removal_target,
            slot_observation: atomic_observation_v2(slot)?,
            registry_observation_sequence: registry_observation_sequence(&state)?,
        };
        validate_absent_route(&state, &binding)?;
        Ok(RouteAbsentSealedV4 {
            binding,
            claim_receipt_digest,
            refence_receipt_digest,
        })
    }
}

fn runtime_evidence_digest(
    value: [u8; 32],
) -> Result<RuntimePendingDrainEvidenceDigestV4, ServingSlotRegistryError> {
    RuntimePendingDrainEvidenceDigestV4::new(value)
        .map_err(|_| ServingSlotRegistryError::V4RegistryMismatch)
}

fn worker_route_witness(
    route: &RuntimeExactLocalRouteIdentityV2,
    lifecycle: SlotLifecycleV1,
) -> SlotRouteWitnessV1 {
    SlotRouteWitnessV1 {
        identity: route.identity.clone(),
        fencing_token: route.controller_fencing_token,
        incarnation: route.route_incarnation,
        lifecycle,
    }
}

fn worker_route_identity(route: &SlotRouteWitnessV1) -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: route.identity.clone(),
        controller_fencing_token: route.fencing_token,
        route_incarnation: route.incarnation,
    }
}

fn routed_port_observation(
    binding: &SealedRouteBindingV4,
) -> Result<RuntimeRoutedSealPortObservationV4, ServingSlotRegistryError> {
    Ok(RuntimeRoutedSealPortObservationV4 {
        registry_lifetime_digest: runtime_evidence_digest(binding.registry_lifetime_digest)?,
        process_instance_id: binding.source_route.identity.process_instance_id.clone(),
        intent_id: binding.intent_id.clone(),
        slot: RuntimeServingSlotV2::from_target(&binding.source_route.identity.target),
        seal_key: *binding.seal_key.as_bytes(),
        seal_generation: binding.seal_generation,
        admission_generation: binding.routed_slot_observation.admission_generation,
        route: worker_route_identity(&binding.source_route),
        slot_observation_sequence: binding.routed_slot_observation.observation_sequence,
        registry_observation_sequence: binding.routed_registry_observation_sequence.as_non_zero(),
        active_guards: u64::from(binding.routed_slot_observation.active_interactions),
    })
}

fn claimed_port_observation(
    source: &RoutedClaimedSealedV4,
) -> Result<RuntimeRoutedClaimedSealPortObservationV4, ServingSlotRegistryError> {
    claimed_port_observation_parts(
        &source.binding,
        source.claim_fence,
        source.claim_receipt_digest,
    )
}

fn claimed_port_observation_parts(
    binding: &SealedRouteBindingV4,
    claim_fence: FencingToken,
    claim_receipt_digest: RegistryDurableReceiptDigestV4,
) -> Result<RuntimeRoutedClaimedSealPortObservationV4, ServingSlotRegistryError> {
    Ok(RuntimeRoutedClaimedSealPortObservationV4 {
        routed: routed_port_observation(binding)?,
        claim_fence,
        claim_receipt_digest: runtime_evidence_digest(*claim_receipt_digest.as_bytes())?,
    })
}

fn local_port_observation(
    source: &LocallyRefencedSealedV4,
) -> Result<RuntimeLocalRefencePortObservationV4, ServingSlotRegistryError> {
    Ok(RuntimeLocalRefencePortObservationV4 {
        claimed: claimed_port_observation_parts(
            &source.binding,
            source.binding.current_route.fencing_token,
            source.claim_receipt_digest,
        )?,
        old_route: worker_route_identity(&source.binding.source_route),
        removal_target: worker_route_identity(&source.binding.current_route),
        provenance: source.provenance.clone(),
        registry_observation_sequence: source.refenced_registry_observation_sequence.as_non_zero(),
        refenced_at: source.refenced_at,
        active_guards: u64::from(source.binding.slot_observation.active_interactions),
    })
}

fn route_absent_port_observation(
    source: &RouteAbsentSealedV4,
) -> Result<RuntimeRouteAbsentPortObservationV4, ServingSlotRegistryError> {
    Ok(RuntimeRouteAbsentPortObservationV4 {
        registry_lifetime_digest: runtime_evidence_digest(source.binding.registry_lifetime_digest)?,
        process_instance_id: source
            .binding
            .source_route
            .identity
            .process_instance_id
            .clone(),
        intent_id: source.binding.intent_id.clone(),
        slot: RuntimeServingSlotV2::from_target(&source.binding.source_route.identity.target),
        seal_key: *source.binding.seal_key.as_bytes(),
        seal_generation: source.binding.seal_generation,
        admission_generation: source.binding.slot_observation.admission_generation,
        source_route: worker_route_identity(&source.binding.source_route),
        removed_route: worker_route_identity(&source.binding.removed_route),
        claim_receipt_digest: runtime_evidence_digest(*source.claim_receipt_digest.as_bytes())?,
        refence_receipt_digest: runtime_evidence_digest(*source.refence_receipt_digest.as_bytes())?,
        slot_observation_sequence: source.binding.slot_observation.observation_sequence,
        registry_observation_sequence: source.binding.registry_observation_sequence.as_non_zero(),
        active_guards: u64::from(source.binding.slot_observation.active_interactions),
    })
}

fn empty_succession_port_observation(
    source: &EmptySuccessionSealedV4,
) -> Result<RuntimeEmptySuccessionPortObservationV4, ServingSlotRegistryError> {
    Ok(RuntimeEmptySuccessionPortObservationV4 {
        registry_lifetime_digest: runtime_evidence_digest(source.registry_lifetime_digest)?,
        process_instance_id: source.successor_identity.process_instance_id.clone(),
        successor_identity: source.successor_identity.clone(),
        intent_id: source.intent_id.clone(),
        slot: RuntimeServingSlotV2::from_target(&source.successor_identity.target),
        seal_key: *source.sealed.seal.seal_key.as_bytes(),
        seal_generation: source.sealed.seal.seal_generation,
        admission_generation: source.sealed.slot_observation.admission_generation,
        predecessor_route: RuntimeExactLocalRouteIdentityV2 {
            identity: source.predecessor_route.identity.clone(),
            controller_fencing_token: source.predecessor_route.source_route_fence,
            route_incarnation: source.predecessor_route.incarnation,
        },
        possible_route_fence_ceiling: source.predecessor_route.possible_route_fence_ceiling,
        successor_fence: source.successor_fence,
        slot_observation_sequence: source.sealed.slot_observation.observation_sequence,
        registry_observation_sequence: source
            .sealed
            .registry_observation
            .observation_sequence()
            .as_non_zero(),
        active_guards: u64::from(source.sealed.slot_observation.active_interactions),
    })
}

fn route_matches_worker(
    route: &SlotRouteWitnessV1,
    identity: &RuntimeProcessIdentityV1,
    fencing_token: FencingToken,
    incarnation: NonZeroU64,
    lifecycle: SlotLifecycleV1,
) -> bool {
    route.identity == *identity
        && route.fencing_token == fencing_token
        && route.incarnation == incarnation
        && route.lifecycle == lifecycle
}

fn ensure_registry_binding(
    registry: &ServingSlotRegistryV1,
    binding: &Weak<RegistryInner>,
) -> Result<(), ServingSlotRegistryError> {
    if Weak::ptr_eq(binding, &Arc::downgrade(&registry.inner)) {
        Ok(())
    } else {
        Err(ServingSlotRegistryError::V4RegistryMismatch)
    }
}

fn registry_observation_sequence(
    state: &RegistryState,
) -> Result<RegistryGlobalObservationSequenceV2, ServingSlotRegistryError> {
    Ok(registry_recovery_observation_v2(state)?.observation_sequence())
}

fn total_active_interactions(slot: &super::SlotCell) -> Result<u64, ServingSlotRegistryError> {
    let mut total = 0_u64;
    for active in slot
        .current
        .iter()
        .chain(slot.staged.iter())
        .chain(slot.retired.values())
        .map(|route| route.active_interactions)
    {
        total = total
            .checked_add(u64::from(active))
            .ok_or(ServingSlotRegistryError::RegistryObservationOverflow)?;
    }
    Ok(total)
}

fn ensure_single_current_route(slot: &super::SlotCell) -> Result<(), ServingSlotRegistryError> {
    if slot.current.is_none() || slot.staged.is_some() || !slot.retired.is_empty() {
        Err(ServingSlotRegistryError::V4RouteMismatch)
    } else {
        Ok(())
    }
}

fn route_matches_witness(route: &RouteRecord, witness: &SlotRouteWitnessV1) -> bool {
    route.route.identity() == &witness.identity
        && route.fencing_token == witness.fencing_token
        && route.incarnation == witness.incarnation
        && route.lifecycle == witness.lifecycle
}

fn validate_routed_observation(
    state: &RegistryState,
    binding: &RoutedObservationBindingV4,
) -> Result<(), ServingSlotRegistryError> {
    if registry_observation_sequence(state)? < binding.registry_observation_sequence {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    let slot = state
        .slots
        .get(&binding.key)
        .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
    ensure_slot_open(slot)?;
    ensure_slot_unsealed(slot)?;
    ensure_single_current_route(slot)?;
    let current = slot
        .current
        .as_ref()
        .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
    if !route_matches_witness(current, &binding.route) {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    if binding.route.lifecycle != SlotLifecycleV1::Serving {
        return Err(ServingSlotRegistryError::V4LifecycleMismatch);
    }
    if atomic_observation_v2(slot)? != binding.slot_observation
        || total_active_interactions(slot)? != binding.total_active_interactions
    {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    ensure_high_water_matches_witness(slot, &binding.route)
}

fn observe_sealed_route_after_guard_releases(
    state: &RegistryState,
    binding: &SealedRouteBindingV4,
    lifecycle: SlotLifecycleV1,
    require_zero: bool,
) -> Result<
    (
        SlotAtomicObservationV2,
        RegistryGlobalObservationSequenceV2,
        u64,
    ),
    ServingSlotRegistryError,
> {
    let registry_observation_sequence = registry_observation_sequence(state)?;
    if registry_observation_sequence < binding.registry_observation_sequence {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    let slot = state
        .slots
        .get(&binding.key)
        .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
    ensure_slot_open(slot)?;
    ensure_single_current_route(slot)?;
    let current = slot
        .current
        .as_ref()
        .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
    if !route_matches_witness(current, &binding.current_route) {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    if current.lifecycle != lifecycle || binding.current_route.lifecycle != lifecycle {
        return Err(ServingSlotRegistryError::V4LifecycleMismatch);
    }
    let seal_matches = slot.seal.as_ref().is_some_and(|seal| {
        seal.seal_key == binding.seal_key
            && seal.seal_generation == binding.seal_generation
            && seal.route.as_ref() == Some(&binding.current_route)
    });
    if !seal_matches {
        return Err(ServingSlotRegistryError::V4CapabilityStale);
    }
    let slot_observation = atomic_observation_v2(slot)?;
    if slot_observation.admission_generation != binding.slot_observation.admission_generation
        || slot_observation.observation_sequence < binding.slot_observation.observation_sequence
    {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    let total_active_interactions = total_active_interactions(slot)?;
    if total_active_interactions > binding.total_active_interactions {
        return Err(ServingSlotRegistryError::V4GuardMismatch);
    }
    if require_zero && total_active_interactions != 0 {
        return Err(ServingSlotRegistryError::V4GuardMismatch);
    }
    ensure_high_water_matches_witness(slot, &binding.current_route)?;
    Ok((
        slot_observation,
        registry_observation_sequence,
        total_active_interactions,
    ))
}

fn validate_sealed_route(
    state: &RegistryState,
    binding: &SealedRouteBindingV4,
    lifecycle: SlotLifecycleV1,
) -> Result<(), ServingSlotRegistryError> {
    if registry_observation_sequence(state)? < binding.registry_observation_sequence {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    let slot = state
        .slots
        .get(&binding.key)
        .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
    ensure_slot_open(slot)?;
    ensure_single_current_route(slot)?;
    let current = slot
        .current
        .as_ref()
        .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
    if !route_matches_witness(current, &binding.current_route) {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    if current.lifecycle != lifecycle || binding.current_route.lifecycle != lifecycle {
        return Err(ServingSlotRegistryError::V4LifecycleMismatch);
    }
    let seal_matches = slot.seal.as_ref().is_some_and(|seal| {
        seal.seal_key == binding.seal_key
            && seal.seal_generation == binding.seal_generation
            && seal.route.as_ref() == Some(&binding.current_route)
    });
    if !seal_matches {
        return Err(ServingSlotRegistryError::V4CapabilityStale);
    }
    if atomic_observation_v2(slot)? != binding.slot_observation
        || total_active_interactions(slot)? != binding.total_active_interactions
    {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    if binding.total_active_interactions != 0 {
        return Err(ServingSlotRegistryError::V4GuardMismatch);
    }
    ensure_high_water_matches_witness(slot, &binding.current_route)
}

fn validate_absent_route(
    state: &RegistryState,
    binding: &AbsentRouteBindingV4,
) -> Result<(), ServingSlotRegistryError> {
    if registry_observation_sequence(state)? < binding.registry_observation_sequence {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    let slot = state
        .slots
        .get(&binding.key)
        .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
    ensure_slot_open(slot)?;
    if selected_route(slot).is_some()
        || !slot.retired.is_empty()
        || total_active_interactions(slot)? != 0
    {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    let seal_matches = slot.seal.as_ref().is_some_and(|seal| {
        seal.seal_key == binding.seal_key
            && seal.seal_generation == binding.seal_generation
            && seal.route.is_none()
    });
    if !seal_matches {
        return Err(ServingSlotRegistryError::V4CapabilityStale);
    }
    if atomic_observation_v2(slot)? != binding.slot_observation {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    ensure_high_water_matches_witness(slot, &binding.removed_route)
}

fn ensure_high_water_matches_witness(
    slot: &super::SlotCell,
    witness: &SlotRouteWitnessV1,
) -> Result<(), ServingSlotRegistryError> {
    let high_water = slot
        .high_water
        .as_ref()
        .ok_or(ServingSlotRegistryError::V4FenceMismatch)?;
    if high_water.identity == witness.identity && high_water.fencing_token == witness.fencing_token
    {
        Ok(())
    } else {
        Err(ServingSlotRegistryError::V4FenceMismatch)
    }
}

fn validate_draining_observation_binding(
    registry: &ServingSlotRegistryV1,
    source: &DrainingRefencedSealedV4,
    observation: &DrainingRefencedObservationV4,
) -> Result<(), ServingSlotRegistryError> {
    ensure_registry_binding(registry, &observation.registry)?;
    if observation.key != source.binding.key
        || observation.seal_key != source.binding.seal_key
        || observation.seal_generation != source.binding.seal_generation
    {
        return Err(ServingSlotRegistryError::V4ObservationMismatch);
    }
    Ok(())
}

fn validate_refence_pair(
    source_route: &SlotRouteWitnessV1,
    removal_target: &SlotRouteWitnessV1,
) -> Result<(), ServingSlotRegistryError> {
    if source_route.identity != removal_target.identity
        || source_route.incarnation != removal_target.incarnation
    {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    if source_route.lifecycle != SlotLifecycleV1::Serving
        || removal_target.lifecycle != SlotLifecycleV1::Serving
    {
        return Err(ServingSlotRegistryError::V4LifecycleMismatch);
    }
    if source_route.fencing_token.next().ok() != Some(removal_target.fencing_token) {
        return Err(ServingSlotRegistryError::V4FenceMismatch);
    }
    Ok(())
}

fn validate_absent_refence_pair(
    source_route: &SlotRouteWitnessV1,
    removal_target: &SlotRouteWitnessV1,
) -> Result<(), ServingSlotRegistryError> {
    if source_route.identity != removal_target.identity
        || source_route.incarnation != removal_target.incarnation
    {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    if source_route.lifecycle != SlotLifecycleV1::Serving
        || removal_target.lifecycle != SlotLifecycleV1::Draining
    {
        return Err(ServingSlotRegistryError::V4LifecycleMismatch);
    }
    if source_route.fencing_token.next().ok() != Some(removal_target.fencing_token) {
        return Err(ServingSlotRegistryError::V4FenceMismatch);
    }
    Ok(())
}

fn validate_recovery_authorization(
    seal: &SlotDrainClaimSealV2,
    intent_seal_key: [u8; 16],
    key: ServingSlotKeyV1,
    identity: RuntimeProcessIdentityV1,
    fencing_token: FencingToken,
    incarnation: NonZeroU64,
) -> Result<(), ServingSlotRegistryError> {
    let source_route = seal
        .route
        .as_ref()
        .ok_or(ServingSlotRegistryError::V4RouteMismatch)?;
    if seal.key != key
        || seal.seal_key.as_bytes() != &intent_seal_key
        || source_route.identity != identity
        || source_route.fencing_token != fencing_token
        || source_route.incarnation != incarnation
        || source_route.lifecycle != SlotLifecycleV1::Serving
    {
        return Err(ServingSlotRegistryError::V4RouteMismatch);
    }
    Ok(())
}

fn recovery_seal_from_binding(binding: SealedRouteBindingV4) -> SlotDrainClaimSealV2 {
    SlotDrainClaimSealV2 {
        registry: binding.registry,
        key: binding.key,
        seal_key: binding.seal_key,
        seal_generation: binding.seal_generation,
        route: Some(binding.source_route),
    }
}

fn recovery_seal_from_absent_binding(binding: AbsentRouteBindingV4) -> SlotDrainClaimSealV2 {
    SlotDrainClaimSealV2 {
        registry: binding.registry,
        key: binding.key,
        seal_key: binding.seal_key,
        seal_generation: binding.seal_generation,
        route: Some(binding.source_route),
    }
}

fn reconstruct_sealed_route_binding(
    registry: &ServingSlotRegistryV1,
    state: &RegistryState,
    seal: &SlotDrainClaimSealV2,
    intent_id: RuntimeDrainIntentIdV2,
    source_route: SlotRouteWitnessV1,
    current_route: SlotRouteWitnessV1,
) -> Result<SealedRouteBindingV4, ServingSlotRegistryError> {
    let slot = state
        .slots
        .get(&seal.key)
        .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
    ensure_slot_open(slot)?;
    ensure_single_current_route(slot)?;
    let current_seal = slot
        .seal
        .as_ref()
        .ok_or(ServingSlotRegistryError::V4CapabilityStale)?;
    let seal_matches = {
        let current = current_seal;
        current.seal_key == seal.seal_key
            && current.seal_generation == seal.seal_generation
            && current.route.as_ref() == Some(&current_route)
            && current.source_route.as_ref() == Some(&source_route)
    };
    if !seal_matches {
        return Err(ServingSlotRegistryError::V4CapabilityStale);
    }
    Ok(SealedRouteBindingV4 {
        registry: Arc::downgrade(&registry.inner),
        registry_lifetime_digest: registry.inner.lifetime_digest,
        intent_id,
        key: seal.key.clone(),
        seal_key: seal.seal_key,
        seal_generation: seal.seal_generation,
        source_route,
        current_route,
        routed_slot_observation: SlotAtomicObservationV2 {
            route: current_seal.source_route.clone(),
            admission_state: SlotAdmissionStateV2::DrainClaimSealed {
                seal_key: current_seal.seal_key,
                seal_generation: current_seal.seal_generation,
            },
            active_interactions: 0,
            admission_generation: current_seal.source_admission_generation,
            observation_sequence: current_seal.source_observation_sequence,
        },
        routed_registry_observation_sequence: current_seal.source_registry_observation_sequence,
        slot_observation: atomic_observation_v2(slot)?,
        registry_observation_sequence: registry_observation_sequence(state)?,
        total_active_interactions: total_active_interactions(slot)?,
    })
}
