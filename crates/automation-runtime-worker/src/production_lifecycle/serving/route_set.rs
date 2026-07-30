use std::fmt::{Debug, Formatter};

use automation_runtime_convergence::ProcessInstanceId;

use crate::{
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRouteSetObservationErrorV2 {
    #[error("runtime route set observation is failed closed")]
    FailedClosed,
    #[error("runtime route set observation sequence is outside the persistence domain")]
    ObservationSequenceOutOfRange,
    #[error("runtime route set retained counts are inconsistent")]
    InconsistentRetainedCounts,
}

pub struct RuntimeRouteSetObservationInputV2 {
    pub process_instance_id: ProcessInstanceId,
    pub registry: RuntimeRegistryRecoveryObservationInputV2,
}

impl Debug for RuntimeRouteSetObservationInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteSetObservationInputV2(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimeRouteSetObservationV2 {
    process_instance_id: ProcessInstanceId,
    registry: RuntimeRegistryRecoveryObservationInputV2,
}

impl RuntimeRouteSetObservationV2 {
    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry.observation_sequence
    }

    pub fn retained_slot_count(&self) -> u64 {
        self.registry.retained_slot_count
    }

    pub fn retained_empty_tombstone_count(&self) -> u64 {
        self.registry.retained_empty_tombstone_count
    }

    pub fn staged_route_count(&self) -> u64 {
        self.registry.staged_route_count
    }

    pub fn serving_route_count(&self) -> u64 {
        self.registry.serving_route_count
    }

    pub fn draining_route_count(&self) -> u64 {
        self.registry.draining_route_count
    }

    pub fn sealed_slot_count(&self) -> u64 {
        self.registry.sealed_slot_count
    }

    pub fn active_interaction_count(&self) -> u64 {
        self.registry.active_interaction_count
    }

    pub fn is_empty(&self) -> bool {
        self.registry.retained_slot_count == self.registry.retained_empty_tombstone_count
            && self.registry.staged_route_count == 0
            && self.registry.serving_route_count == 0
            && self.registry.draining_route_count == 0
            && self.registry.sealed_slot_count == 0
            && self.registry.active_interaction_count == 0
    }
}

impl Debug for RuntimeRouteSetObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteSetObservationV2(<redacted>)")
    }
}

pub fn accept_runtime_route_set_observation_v2(
    input: RuntimeRouteSetObservationInputV2,
) -> Result<RuntimeRouteSetObservationV2, RuntimeRouteSetObservationErrorV2> {
    let registry = input.registry;
    if registry.registry_failed_closed || registry.failed_closed_slot_count != 0 {
        return Err(RuntimeRouteSetObservationErrorV2::FailedClosed);
    }
    if registry.observation_sequence.get() > i64::MAX as u64 {
        return Err(RuntimeRouteSetObservationErrorV2::ObservationSequenceOutOfRange);
    }
    if registry.retained_empty_tombstone_count > registry.retained_slot_count {
        return Err(RuntimeRouteSetObservationErrorV2::InconsistentRetainedCounts);
    }
    let retained_nonempty_slot_count = registry
        .retained_slot_count
        .checked_sub(registry.retained_empty_tombstone_count)
        .ok_or(RuntimeRouteSetObservationErrorV2::InconsistentRetainedCounts)?;
    let represented_route_count = registry
        .staged_route_count
        .checked_add(registry.serving_route_count)
        .and_then(|count| count.checked_add(registry.draining_route_count))
        .and_then(|count| count.checked_add(registry.sealed_slot_count))
        .ok_or(RuntimeRouteSetObservationErrorV2::InconsistentRetainedCounts)?;
    if registry.staged_route_count > retained_nonempty_slot_count
        || registry.serving_route_count > retained_nonempty_slot_count
        || registry.sealed_slot_count > retained_nonempty_slot_count
        || represented_route_count < retained_nonempty_slot_count
        || (registry.draining_route_count != 0 && retained_nonempty_slot_count == 0)
        || (registry.active_interaction_count != 0
            && registry.serving_route_count == 0
            && registry.draining_route_count == 0)
    {
        return Err(RuntimeRouteSetObservationErrorV2::InconsistentRetainedCounts);
    }
    let no_routes = registry.staged_route_count == 0
        && registry.serving_route_count == 0
        && registry.draining_route_count == 0
        && registry.sealed_slot_count == 0
        && registry.active_interaction_count == 0;
    if no_routes && registry.retained_slot_count != registry.retained_empty_tombstone_count {
        return Err(RuntimeRouteSetObservationErrorV2::InconsistentRetainedCounts);
    }
    Ok(RuntimeRouteSetObservationV2 {
        process_instance_id: input.process_instance_id,
        registry,
    })
}
