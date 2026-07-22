use std::num::NonZeroU64;

use chrono::{DateTime, Utc};

use crate::RuntimeCutoverCoordinatorIdV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeWriterFenceGenerationV1(NonZeroU64);

impl RuntimeWriterFenceGenerationV1 {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn into_non_zero(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeWriterFenceClosedLeaseIdV1 {
    pub generation: RuntimeWriterFenceGenerationV1,
    pub coordinator_id: RuntimeCutoverCoordinatorIdV1,
    pub lease_epoch: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObservedWriterFenceClosedV1 {
    pub lease_id: RuntimeWriterFenceClosedLeaseIdV1,
    pub observed_database_now: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl RuntimeObservedWriterFenceClosedV1 {
    pub fn current_lease(&self) -> Option<&RuntimeWriterFenceClosedLeaseIdV1> {
        (self.expires_at > self.observed_database_now).then_some(&self.lease_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeWriterFenceObservationV1 {
    Open {
        generation: RuntimeWriterFenceGenerationV1,
        observed_database_now: DateTime<Utc>,
    },
    Closed(RuntimeObservedWriterFenceClosedV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeObserveWriterFenceV1;

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use chrono::{DateTime, Utc};

    use super::{
        RuntimeObserveWriterFenceV1, RuntimeObservedWriterFenceClosedV1,
        RuntimeWriterFenceClosedLeaseIdV1, RuntimeWriterFenceGenerationV1,
        RuntimeWriterFenceObservationV1,
    };
    use crate::RuntimeCutoverCoordinatorIdV1;

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn generation(value: u64) -> RuntimeWriterFenceGenerationV1 {
        RuntimeWriterFenceGenerationV1::new(non_zero(value))
    }

    fn lease_id() -> RuntimeWriterFenceClosedLeaseIdV1 {
        RuntimeWriterFenceClosedLeaseIdV1 {
            generation: generation(7),
            coordinator_id: RuntimeCutoverCoordinatorIdV1::parse(
                "00112233445566778899aabbccddeeff",
            )
            .unwrap(),
            lease_epoch: non_zero(11),
        }
    }

    #[test]
    fn generation_preserves_the_nonzero_persistence_domain() {
        let value = non_zero(7);
        let generation = RuntimeWriterFenceGenerationV1::new(value);

        assert_eq!(generation.get(), 7);
        assert_eq!(generation.into_non_zero(), value);
    }

    #[test]
    fn open_observation_carries_one_exact_generation_and_database_clock() {
        let observation = RuntimeWriterFenceObservationV1::Open {
            generation: generation(7),
            observed_database_now: at(100),
        };

        assert_eq!(
            observation,
            RuntimeWriterFenceObservationV1::Open {
                generation: generation(7),
                observed_database_now: at(100),
            }
        );
        assert_eq!(RuntimeObserveWriterFenceV1, RuntimeObserveWriterFenceV1);
    }

    #[test]
    fn closed_observation_exposes_only_a_fresh_exact_lease_as_current() {
        let fresh = RuntimeObservedWriterFenceClosedV1 {
            lease_id: lease_id(),
            observed_database_now: at(100),
            expires_at: at(130),
        };

        assert_eq!(fresh.current_lease(), Some(&lease_id()));
        assert_eq!(
            RuntimeWriterFenceObservationV1::Closed(fresh.clone()),
            RuntimeWriterFenceObservationV1::Closed(fresh)
        );
    }

    #[test]
    fn expired_closed_observation_remains_closed_without_current_authority() {
        for observed_database_now in [130, 131] {
            let expired = RuntimeObservedWriterFenceClosedV1 {
                lease_id: lease_id(),
                observed_database_now: at(observed_database_now),
                expires_at: at(130),
            };
            let observation = RuntimeWriterFenceObservationV1::Closed(expired.clone());

            assert_eq!(expired.current_lease(), None);
            assert_eq!(
                observation,
                RuntimeWriterFenceObservationV1::Closed(expired)
            );
        }
    }
}
