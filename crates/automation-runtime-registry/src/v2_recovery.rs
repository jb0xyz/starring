use std::fmt;
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegistryGlobalObservationSequenceV2(NonZeroU64);

impl RegistryGlobalObservationSequenceV2 {
    pub(crate) const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn as_non_zero(self) -> NonZeroU64 {
        self.0
    }

    pub(crate) const fn value(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RegistryRecoveryObservationV2 {
    observation_sequence: RegistryGlobalObservationSequenceV2,
    retained_slot_count: u64,
    retained_empty_tombstone_count: u64,
    staged_route_count: u64,
    serving_route_count: u64,
    draining_route_count: u64,
    sealed_slot_count: u64,
    active_interaction_count: u64,
    failed_closed_slot_count: u64,
    registry_failed_closed: bool,
}

pub(crate) struct RegistryRecoveryObservationPartsV2 {
    pub(crate) observation_sequence: RegistryGlobalObservationSequenceV2,
    pub(crate) retained_slot_count: u64,
    pub(crate) retained_empty_tombstone_count: u64,
    pub(crate) staged_route_count: u64,
    pub(crate) serving_route_count: u64,
    pub(crate) draining_route_count: u64,
    pub(crate) sealed_slot_count: u64,
    pub(crate) active_interaction_count: u64,
    pub(crate) failed_closed_slot_count: u64,
    pub(crate) registry_failed_closed: bool,
}

impl RegistryRecoveryObservationV2 {
    pub(crate) const fn new(parts: RegistryRecoveryObservationPartsV2) -> Self {
        Self {
            observation_sequence: parts.observation_sequence,
            retained_slot_count: parts.retained_slot_count,
            retained_empty_tombstone_count: parts.retained_empty_tombstone_count,
            staged_route_count: parts.staged_route_count,
            serving_route_count: parts.serving_route_count,
            draining_route_count: parts.draining_route_count,
            sealed_slot_count: parts.sealed_slot_count,
            active_interaction_count: parts.active_interaction_count,
            failed_closed_slot_count: parts.failed_closed_slot_count,
            registry_failed_closed: parts.registry_failed_closed,
        }
    }

    pub const fn observation_sequence(self) -> RegistryGlobalObservationSequenceV2 {
        self.observation_sequence
    }

    pub const fn staged_route_count(self) -> u64 {
        self.staged_route_count
    }

    pub const fn retained_slot_count(self) -> u64 {
        self.retained_slot_count
    }

    pub const fn retained_empty_tombstone_count(self) -> u64 {
        self.retained_empty_tombstone_count
    }

    pub const fn serving_route_count(self) -> u64 {
        self.serving_route_count
    }

    pub const fn draining_route_count(self) -> u64 {
        self.draining_route_count
    }

    pub const fn sealed_slot_count(self) -> u64 {
        self.sealed_slot_count
    }

    pub const fn active_interaction_count(self) -> u64 {
        self.active_interaction_count
    }

    pub const fn failed_closed_slot_count(self) -> u64 {
        self.failed_closed_slot_count
    }

    pub const fn registry_failed_closed(self) -> bool {
        self.registry_failed_closed
    }

    pub const fn is_recovery_empty(self) -> bool {
        !self.registry_failed_closed
            && self.staged_route_count == 0
            && self.serving_route_count == 0
            && self.draining_route_count == 0
            && self.sealed_slot_count == 0
            && self.active_interaction_count == 0
            && self.failed_closed_slot_count == 0
    }
}

impl fmt::Debug for RegistryRecoveryObservationV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RegistryRecoveryObservationV2(<redacted>)")
    }
}
