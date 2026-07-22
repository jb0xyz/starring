use std::num::NonZeroU64;

use crate::SlotRouteWitnessV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotSealKeyV2([u8; 16]);

impl SlotSealKeyV2 {
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl TryFrom<&[u8]> for SlotSealKeyV2 {
    type Error = SlotSealKeyErrorV2;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let bytes = <[u8; 16]>::try_from(value).map_err(|_| SlotSealKeyErrorV2::InvalidLength)?;
        Ok(Self(bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SlotSealKeyErrorV2 {
    #[error("slot seal key must contain exactly sixteen bytes")]
    InvalidLength,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotAdmissionStateV2 {
    Empty,
    Staged,
    Serving,
    DrainClaimSealed {
        seal_key: SlotSealKeyV2,
        seal_generation: NonZeroU64,
    },
    Draining,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotAtomicObservationV2 {
    pub route: Option<SlotRouteWitnessV1>,
    pub admission_state: SlotAdmissionStateV2,
    pub active_interactions: u32,
    pub admission_generation: NonZeroU64,
    pub observation_sequence: NonZeroU64,
}
