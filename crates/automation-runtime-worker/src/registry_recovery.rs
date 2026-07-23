use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_convergence::ProcessInstanceId;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeRegistryGlobalObservationSequenceV2(NonZeroU64);

impl RuntimeRegistryGlobalObservationSequenceV2 {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn as_non_zero(self) -> NonZeroU64 {
        self.0
    }
}

impl Debug for RuntimeRegistryGlobalObservationSequenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryGlobalObservationSequenceV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RuntimeRegistryRecoveryObservationInputV2 {
    pub observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    pub retained_slot_count: u64,
    pub retained_empty_tombstone_count: u64,
    pub staged_route_count: u64,
    pub serving_route_count: u64,
    pub draining_route_count: u64,
    pub sealed_slot_count: u64,
    pub active_interaction_count: u64,
    pub failed_closed_slot_count: u64,
    pub registry_failed_closed: bool,
}

impl Debug for RuntimeRegistryRecoveryObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryRecoveryObservationInputV2(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimeRegistryRecoveryEmptyObservationV2 {
    process_instance_id: ProcessInstanceId,
    observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    retained_slot_count: u64,
    retained_empty_tombstone_count: u64,
}

impl RuntimeRegistryRecoveryEmptyObservationV2 {
    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.observation_sequence
    }

    pub fn retained_slot_count(&self) -> u64 {
        self.retained_slot_count
    }

    pub fn retained_empty_tombstone_count(&self) -> u64 {
        self.retained_empty_tombstone_count
    }
}

impl Debug for RuntimeRegistryRecoveryEmptyObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryRecoveryEmptyObservationV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRegistryRecoveryObservationErrorV2 {
    #[error("runtime registry recovery observation is failed closed")]
    FailedClosed,
    #[error("runtime registry recovery observation is not empty")]
    NotEmpty,
    #[error("runtime registry recovery retained counts are inconsistent")]
    InconsistentRetainedCounts,
}

pub fn accept_runtime_registry_recovery_empty_observation_v2(
    process_instance_id: ProcessInstanceId,
    observation: RuntimeRegistryRecoveryObservationInputV2,
) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV2> {
    if observation.registry_failed_closed
        || observation.failed_closed_slot_count != 0
        || observation.observation_sequence.get() == u64::MAX
    {
        return Err(RuntimeRegistryRecoveryObservationErrorV2::FailedClosed);
    }
    if observation.staged_route_count != 0
        || observation.serving_route_count != 0
        || observation.draining_route_count != 0
        || observation.sealed_slot_count != 0
        || observation.active_interaction_count != 0
    {
        return Err(RuntimeRegistryRecoveryObservationErrorV2::NotEmpty);
    }
    if observation.retained_slot_count != observation.retained_empty_tombstone_count {
        return Err(RuntimeRegistryRecoveryObservationErrorV2::InconsistentRetainedCounts);
    }
    Ok(RuntimeRegistryRecoveryEmptyObservationV2 {
        process_instance_id,
        observation_sequence: observation.observation_sequence,
        retained_slot_count: observation.retained_slot_count,
        retained_empty_tombstone_count: observation.retained_empty_tombstone_count,
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_convergence::ProcessInstanceId;

    use super::{
        accept_runtime_registry_recovery_empty_observation_v2,
        RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationErrorV2,
        RuntimeRegistryRecoveryObservationInputV2,
    };

    fn sequence(value: u64) -> RuntimeRegistryGlobalObservationSequenceV2 {
        RuntimeRegistryGlobalObservationSequenceV2::new(NonZeroU64::new(value).unwrap())
    }

    fn input() -> RuntimeRegistryRecoveryObservationInputV2 {
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: sequence(9),
            retained_slot_count: 3,
            retained_empty_tombstone_count: 3,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        }
    }

    fn process() -> ProcessInstanceId {
        ProcessInstanceId::parse("runtime-process:1").unwrap()
    }

    #[test]
    fn accepts_empty_registry_with_high_water_tombstones() {
        let accepted =
            accept_runtime_registry_recovery_empty_observation_v2(process(), input()).unwrap();

        assert_eq!(accepted.process_instance_id().as_str(), "runtime-process:1");
        assert_eq!(accepted.observation_sequence().get(), 9);
        assert_eq!(accepted.observation_sequence().as_non_zero().get(), 9);
        assert_eq!(accepted.retained_slot_count(), 3);
        assert_eq!(accepted.retained_empty_tombstone_count(), 3);
        assert_eq!(
            format!("{accepted:?}"),
            "RuntimeRegistryRecoveryEmptyObservationV2(<redacted>)"
        );
    }

    #[test]
    fn rejects_every_live_registry_blocker_as_not_empty() {
        for observation in [
            RuntimeRegistryRecoveryObservationInputV2 {
                staged_route_count: 1,
                ..input()
            },
            RuntimeRegistryRecoveryObservationInputV2 {
                serving_route_count: 1,
                ..input()
            },
            RuntimeRegistryRecoveryObservationInputV2 {
                draining_route_count: 1,
                ..input()
            },
            RuntimeRegistryRecoveryObservationInputV2 {
                sealed_slot_count: 1,
                ..input()
            },
            RuntimeRegistryRecoveryObservationInputV2 {
                active_interaction_count: 1,
                ..input()
            },
        ] {
            assert_eq!(
                accept_runtime_registry_recovery_empty_observation_v2(process(), observation),
                Err(RuntimeRegistryRecoveryObservationErrorV2::NotEmpty)
            );
        }
    }

    #[test]
    fn rejects_slot_and_registry_failures() {
        for observation in [
            RuntimeRegistryRecoveryObservationInputV2 {
                failed_closed_slot_count: 1,
                ..input()
            },
            RuntimeRegistryRecoveryObservationInputV2 {
                registry_failed_closed: true,
                ..input()
            },
            RuntimeRegistryRecoveryObservationInputV2 {
                observation_sequence: sequence(u64::MAX),
                ..input()
            },
        ] {
            assert_eq!(
                accept_runtime_registry_recovery_empty_observation_v2(process(), observation),
                Err(RuntimeRegistryRecoveryObservationErrorV2::FailedClosed)
            );
        }
    }

    #[test]
    fn rejects_unclassified_retained_slots() {
        let observation = RuntimeRegistryRecoveryObservationInputV2 {
            retained_empty_tombstone_count: 2,
            ..input()
        };

        assert_eq!(
            accept_runtime_registry_recovery_empty_observation_v2(process(), observation),
            Err(RuntimeRegistryRecoveryObservationErrorV2::InconsistentRetainedCounts)
        );
    }

    #[test]
    fn sequence_and_input_debug_are_redacted() {
        let observation = input();

        assert_eq!(observation.observation_sequence.get(), 9);
        assert_eq!(
            format!("{:?}", observation.observation_sequence),
            "RuntimeRegistryGlobalObservationSequenceV2(<redacted>)"
        );
        assert_eq!(
            format!("{observation:?}"),
            "RuntimeRegistryRecoveryObservationInputV2(<redacted>)"
        );
    }
}
