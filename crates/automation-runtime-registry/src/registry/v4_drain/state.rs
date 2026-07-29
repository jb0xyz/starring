use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegistryDurableReceiptDigestV4([u8; 32]);

impl RegistryDurableReceiptDigestV4 {
    pub(super) fn from_checked_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for RegistryDurableReceiptDigestV4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RegistryDurableReceiptDigestV4(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PreviousRouteEnvelopeV4 {
    pub(super) key: ServingSlotKeyV1,
    pub(super) identity: RuntimeProcessIdentityV1,
    pub(super) incarnation: NonZeroU64,
    pub(super) source_route_fence: FencingToken,
    pub(super) possible_route_fence_ceiling: FencingToken,
}

impl std::fmt::Debug for PreviousRouteEnvelopeV4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreviousRouteEnvelopeV4(<redacted>)")
    }
}

impl PreviousRouteEnvelopeV4 {
    pub fn new(
        key: ServingSlotKeyV1,
        identity: RuntimeProcessIdentityV1,
        incarnation: NonZeroU64,
        source_route_fence: FencingToken,
        possible_route_fence_ceiling: FencingToken,
    ) -> Result<Self, ServingSlotRegistryError> {
        if !key.matches_target(&identity.target) {
            return Err(ServingSlotRegistryError::V4RouteMismatch);
        }
        if source_route_fence.next().ok() != Some(possible_route_fence_ceiling) {
            return Err(ServingSlotRegistryError::V4FenceMismatch);
        }
        Ok(Self {
            key,
            identity,
            incarnation,
            source_route_fence,
            possible_route_fence_ceiling,
        })
    }

    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.key
    }

    pub fn identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.identity
    }

    pub const fn incarnation(&self) -> NonZeroU64 {
        self.incarnation
    }

    pub const fn source_route_fence(&self) -> FencingToken {
        self.source_route_fence
    }

    pub const fn possible_route_fence_ceiling(&self) -> FencingToken {
        self.possible_route_fence_ceiling
    }
}

pub(super) struct RoutedObservationBindingV4 {
    pub(super) registry: Weak<RegistryInner>,
    pub(super) key: ServingSlotKeyV1,
    pub(super) route: SlotRouteWitnessV1,
    pub(super) slot_observation: SlotAtomicObservationV2,
    pub(super) registry_observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(super) total_active_interactions: u64,
}

pub(super) struct SealedRouteBindingV4 {
    pub(super) registry: Weak<RegistryInner>,
    pub(super) registry_lifetime_digest: [u8; 32],
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) key: ServingSlotKeyV1,
    pub(super) seal_key: SlotSealKeyV2,
    pub(super) seal_generation: NonZeroU64,
    pub(super) source_route: SlotRouteWitnessV1,
    pub(super) current_route: SlotRouteWitnessV1,
    pub(super) routed_slot_observation: SlotAtomicObservationV2,
    pub(super) routed_registry_observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(super) slot_observation: SlotAtomicObservationV2,
    pub(super) registry_observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(super) total_active_interactions: u64,
}

pub(super) struct AbsentRouteBindingV4 {
    pub(super) registry: Weak<RegistryInner>,
    pub(super) registry_lifetime_digest: [u8; 32],
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) key: ServingSlotKeyV1,
    pub(super) seal_key: SlotSealKeyV2,
    pub(super) seal_generation: NonZeroU64,
    pub(super) source_route: SlotRouteWitnessV1,
    pub(super) removed_route: SlotRouteWitnessV1,
    pub(super) slot_observation: SlotAtomicObservationV2,
    pub(super) registry_observation_sequence: RegistryGlobalObservationSequenceV2,
}

pub struct RoutedObservedV4 {
    pub(super) binding: RoutedObservationBindingV4,
}

impl RoutedObservedV4 {
    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.binding.key
    }

    pub fn route(&self) -> &SlotRouteWitnessV1 {
        &self.binding.route
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.binding.slot_observation
    }

    pub const fn registry_observation_sequence(&self) -> RegistryGlobalObservationSequenceV2 {
        self.binding.registry_observation_sequence
    }
}

pub struct RoutedSealedV4 {
    pub(super) binding: SealedRouteBindingV4,
}

impl RoutedSealedV4 {
    pub const fn registry_lifetime_digest(&self) -> &[u8; 32] {
        &self.binding.registry_lifetime_digest
    }

    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.binding.key
    }

    pub const fn seal_key(&self) -> SlotSealKeyV2 {
        self.binding.seal_key
    }

    pub const fn seal_generation(&self) -> NonZeroU64 {
        self.binding.seal_generation
    }

    pub fn route(&self) -> &SlotRouteWitnessV1 {
        &self.binding.current_route
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.binding.slot_observation
    }
}

pub struct RoutedSealedObservationV4 {
    pub(super) registry: Weak<RegistryInner>,
    pub(super) key: ServingSlotKeyV1,
    pub(super) seal_key: SlotSealKeyV2,
    pub(super) seal_generation: NonZeroU64,
    pub(super) slot_observation: SlotAtomicObservationV2,
    pub(super) registry_observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(super) total_active_interactions: u64,
}

impl RoutedSealedObservationV4 {
    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.key
    }

    pub const fn seal_key(&self) -> SlotSealKeyV2 {
        self.seal_key
    }

    pub const fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.slot_observation
    }

    pub const fn registry_observation_sequence(&self) -> RegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub const fn total_active_interactions(&self) -> u64 {
        self.total_active_interactions
    }

    pub fn belongs_to(&self, registry: &ServingSlotRegistryV1) -> bool {
        Weak::ptr_eq(&self.registry, &Arc::downgrade(&registry.inner))
    }
}

pub struct RoutedClaimedSealedV4 {
    pub(super) binding: SealedRouteBindingV4,
    pub(super) claim_fence: FencingToken,
    pub(super) claim_receipt_digest: RegistryDurableReceiptDigestV4,
}

impl RoutedClaimedSealedV4 {
    pub fn source_route(&self) -> &SlotRouteWitnessV1 {
        &self.binding.source_route
    }

    pub const fn claim_fence(&self) -> FencingToken {
        self.claim_fence
    }

    pub const fn claim_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.claim_receipt_digest
    }
}

pub struct LocallyRefencedSealedV4 {
    pub(super) binding: SealedRouteBindingV4,
    pub(super) claim_receipt_digest: RegistryDurableReceiptDigestV4,
    pub(super) provenance: RuntimeRouteMutationProvenanceV2,
    pub(super) refenced_at: DateTime<Utc>,
    pub(super) refenced_registry_observation_sequence: RegistryGlobalObservationSequenceV2,
}

impl LocallyRefencedSealedV4 {
    pub const fn registry_lifetime_digest(&self) -> &[u8; 32] {
        &self.binding.registry_lifetime_digest
    }

    pub fn old_route(&self) -> &SlotRouteWitnessV1 {
        &self.binding.source_route
    }

    pub fn removal_target(&self) -> &SlotRouteWitnessV1 {
        &self.binding.current_route
    }

    pub const fn claim_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.claim_receipt_digest
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.binding.slot_observation
    }
}

pub struct DurablyRefencedSealedV4 {
    pub(super) binding: SealedRouteBindingV4,
    pub(super) claim_receipt_digest: RegistryDurableReceiptDigestV4,
    pub(super) refence_receipt_digest: RegistryDurableReceiptDigestV4,
}

impl DurablyRefencedSealedV4 {
    pub const fn registry_lifetime_digest(&self) -> &[u8; 32] {
        &self.binding.registry_lifetime_digest
    }

    pub fn removal_target(&self) -> &SlotRouteWitnessV1 {
        &self.binding.current_route
    }

    pub const fn claim_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.claim_receipt_digest
    }

    pub const fn refence_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.refence_receipt_digest
    }
}

pub struct DrainingRefencedSealedV4 {
    pub(super) binding: SealedRouteBindingV4,
    pub(super) claim_receipt_digest: RegistryDurableReceiptDigestV4,
    pub(super) refence_receipt_digest: RegistryDurableReceiptDigestV4,
}

impl DrainingRefencedSealedV4 {
    pub fn removal_target(&self) -> &SlotRouteWitnessV1 {
        &self.binding.current_route
    }

    pub const fn refence_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.refence_receipt_digest
    }
}

pub struct DrainingRefencedObservationV4 {
    pub(super) registry: Weak<RegistryInner>,
    pub(super) key: ServingSlotKeyV1,
    pub(super) seal_key: SlotSealKeyV2,
    pub(super) seal_generation: NonZeroU64,
    pub(super) slot_observation: SlotAtomicObservationV2,
    pub(super) registry_observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(super) total_active_interactions: u64,
}

impl DrainingRefencedObservationV4 {
    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.slot_observation
    }

    pub const fn total_active_interactions(&self) -> u64 {
        self.total_active_interactions
    }
}

pub struct RouteAbsentSealedV4 {
    pub(super) binding: AbsentRouteBindingV4,
    pub(super) claim_receipt_digest: RegistryDurableReceiptDigestV4,
    pub(super) refence_receipt_digest: RegistryDurableReceiptDigestV4,
}

impl RouteAbsentSealedV4 {
    pub const fn registry_lifetime_digest(&self) -> &[u8; 32] {
        &self.binding.registry_lifetime_digest
    }

    pub fn source_route(&self) -> &SlotRouteWitnessV1 {
        &self.binding.source_route
    }

    pub fn removed_route(&self) -> &SlotRouteWitnessV1 {
        &self.binding.removed_route
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.binding.slot_observation
    }

    pub const fn refence_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.refence_receipt_digest
    }
}

pub struct EmptySuccessionSealedV4 {
    pub(super) sealed: SealedEmptyRecoveryDrainClaimV2,
    pub(super) registry_lifetime_digest: [u8; 32],
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) predecessor_route: PreviousRouteEnvelopeV4,
    pub(super) successor_identity: RuntimeProcessIdentityV1,
    pub(super) successor_fence: FencingToken,
}

impl EmptySuccessionSealedV4 {
    pub const fn registry_lifetime_digest(&self) -> &[u8; 32] {
        &self.registry_lifetime_digest
    }

    pub fn predecessor_route(&self) -> &PreviousRouteEnvelopeV4 {
        &self.predecessor_route
    }

    pub fn successor_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.successor_identity
    }

    pub const fn successor_fence(&self) -> FencingToken {
        self.successor_fence
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        self.sealed.slot_observation()
    }
}

pub struct AcknowledgedEmptyV4 {
    pub(super) registry: Weak<RegistryInner>,
    pub(super) key: ServingSlotKeyV1,
    pub(super) slot_observation: SlotAtomicObservationV2,
    pub(super) registry_observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(super) claim_receipt_digest: Option<RegistryDurableReceiptDigestV4>,
    pub(super) refence_receipt_digest: Option<RegistryDurableReceiptDigestV4>,
    pub(super) acknowledgement_receipt_digest: RegistryDurableReceiptDigestV4,
    pub(super) successor_fence: FencingToken,
}

impl AcknowledgedEmptyV4 {
    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.key
    }

    pub fn slot_observation(&self) -> &SlotAtomicObservationV2 {
        &self.slot_observation
    }

    pub const fn registry_observation_sequence(&self) -> RegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub const fn claim_receipt_digest(&self) -> Option<RegistryDurableReceiptDigestV4> {
        self.claim_receipt_digest
    }

    pub const fn refence_receipt_digest(&self) -> Option<RegistryDurableReceiptDigestV4> {
        self.refence_receipt_digest
    }

    pub const fn acknowledgement_receipt_digest(&self) -> RegistryDurableReceiptDigestV4 {
        self.acknowledgement_receipt_digest
    }

    pub const fn successor_fence(&self) -> FencingToken {
        self.successor_fence
    }

    pub fn belongs_to(&self, registry: &ServingSlotRegistryV1) -> bool {
        Weak::ptr_eq(&self.registry, &Arc::downgrade(&registry.inner))
    }
}
