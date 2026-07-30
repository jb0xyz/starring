use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use automation_runtime_controller::RuntimeServingSlotV2;
use automation_runtime_convergence::ProcessInstanceId;

use super::RuntimeRouteSetEpochV2;
use crate::{RuntimeGatewayCoordinatorGenerationV2, RuntimeRegistryGlobalObservationSequenceV2};

const MAX_SERVING_SLOT_WORK_V2: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeServingOpenSupervisorConfigErrorV2 {
    #[error("runtime serving supervisor capacity exceeds its supported domain")]
    CapacityOutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeServingOpenSupervisorConfigV2 {
    max_in_flight: NonZeroUsize,
}

impl RuntimeServingOpenSupervisorConfigV2 {
    pub fn new(
        max_in_flight: NonZeroUsize,
    ) -> Result<Self, RuntimeServingOpenSupervisorConfigErrorV2> {
        if max_in_flight.get() > MAX_SERVING_SLOT_WORK_V2 {
            return Err(RuntimeServingOpenSupervisorConfigErrorV2::CapacityOutOfRange);
        }
        Ok(Self { max_in_flight })
    }

    pub fn max_in_flight(self) -> NonZeroUsize {
        self.max_in_flight
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeServingSlotWorkErrorV2 {
    #[error("runtime serving slot work route set epoch is stale")]
    StaleRouteSetEpoch,
    #[error("runtime serving slot already has active work")]
    SlotAlreadyActive,
    #[error("runtime serving slot work capacity is exhausted")]
    CapacityExhausted,
    #[error("runtime serving slot work supervisor is sealed")]
    SupervisorSealed,
    #[error("runtime serving slot work permit is stale")]
    StalePermit,
    #[error("runtime serving slot work sequence is exhausted")]
    SequenceExhausted,
}

pub struct RuntimeServingSlotWorkRequestV2 {
    slot: RuntimeServingSlotV2,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
}

impl RuntimeServingSlotWorkRequestV2 {
    pub(super) fn new(
        epoch: &RuntimeRouteSetEpochV2,
        route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
        slot: RuntimeServingSlotV2,
    ) -> Self {
        Self {
            slot,
            coordinator_generation: epoch.coordinator_generation(),
            process_instance_id: epoch.process_instance_id().clone(),
            route_set_sequence,
        }
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub(super) fn into_slot(
        self,
        epoch: &RuntimeRouteSetEpochV2,
        route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    ) -> Result<RuntimeServingSlotV2, RuntimeServingSlotWorkErrorV2> {
        if self.coordinator_generation != epoch.coordinator_generation()
            || &self.process_instance_id != epoch.process_instance_id()
            || self.route_set_sequence != route_set_sequence
        {
            Err(RuntimeServingSlotWorkErrorV2::StaleRouteSetEpoch)
        } else {
            Ok(self.slot)
        }
    }
}

impl Debug for RuntimeServingSlotWorkRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingSlotWorkRequestV2(<redacted>)")
    }
}

struct RuntimeServingSlotWorkPermitIdentityV2 {
    slot: RuntimeServingSlotV2,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    work_sequence: NonZeroU64,
}

pub struct RuntimeServingSlotWorkPermitV2 {
    identity: Option<RuntimeServingSlotWorkPermitIdentityV2>,
    supervisor: Weak<Mutex<RuntimeServingSlotWorkStateV2>>,
}

impl RuntimeServingSlotWorkPermitV2 {
    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self
            .identity
            .as_ref()
            .expect("live serving slot permit must retain identity")
            .slot
    }

    pub fn route_set_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.identity
            .as_ref()
            .expect("live serving slot permit must retain identity")
            .route_set_sequence
    }

    pub fn ensure_active(&self) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(RuntimeServingSlotWorkErrorV2::StalePermit)?;
        let supervisor = self
            .supervisor
            .upgrade()
            .ok_or(RuntimeServingSlotWorkErrorV2::StalePermit)?;
        let state = lock_state(&supervisor);
        if state.sealed {
            return Err(RuntimeServingSlotWorkErrorV2::SupervisorSealed);
        }
        if state.active.get(&identity.slot) != Some(&identity.work_sequence) {
            return Err(RuntimeServingSlotWorkErrorV2::StalePermit);
        }
        Ok(())
    }
}

impl Debug for RuntimeServingSlotWorkPermitV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeServingSlotWorkPermitV2(<redacted>)")
    }
}

impl Drop for RuntimeServingSlotWorkPermitV2 {
    fn drop(&mut self) {
        let Some(identity) = self.identity.take() else {
            return;
        };
        let Some(supervisor) = self.supervisor.upgrade() else {
            return;
        };
        let mut state = lock_state(&supervisor);
        if state.active.get(&identity.slot) == Some(&identity.work_sequence) {
            state.active.remove(&identity.slot);
        }
    }
}

struct RuntimeServingSlotWorkStateV2 {
    max_in_flight: NonZeroUsize,
    next_work_sequence: u64,
    active: BTreeMap<RuntimeServingSlotV2, NonZeroU64>,
    sealed: bool,
}

pub(super) struct RuntimeServingSlotWorkSupervisorV2 {
    state: Arc<Mutex<RuntimeServingSlotWorkStateV2>>,
}

impl RuntimeServingSlotWorkSupervisorV2 {
    pub(super) fn new(config: RuntimeServingOpenSupervisorConfigV2) -> Self {
        Self {
            state: Arc::new(Mutex::new(RuntimeServingSlotWorkStateV2 {
                max_in_flight: config.max_in_flight,
                next_work_sequence: 0,
                active: BTreeMap::new(),
                sealed: false,
            })),
        }
    }

    pub(super) fn active_count(&self) -> usize {
        lock_state(&self.state).active.len()
    }

    pub(super) fn seal(&mut self) {
        let mut state = lock_state(&self.state);
        state.sealed = true;
        state.active.clear();
    }

    pub(super) fn begin(
        &mut self,
        epoch: &RuntimeRouteSetEpochV2,
        route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
        slot: RuntimeServingSlotV2,
    ) -> Result<RuntimeServingSlotWorkPermitV2, RuntimeServingSlotWorkErrorV2> {
        let mut state = lock_state(&self.state);
        if state.sealed {
            return Err(RuntimeServingSlotWorkErrorV2::SupervisorSealed);
        }
        if state.active.contains_key(&slot) {
            return Err(RuntimeServingSlotWorkErrorV2::SlotAlreadyActive);
        }
        if state.active.len() >= state.max_in_flight.get() {
            return Err(RuntimeServingSlotWorkErrorV2::CapacityExhausted);
        }
        let sequence = state
            .next_work_sequence
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .and_then(NonZeroU64::new)
            .ok_or(RuntimeServingSlotWorkErrorV2::SequenceExhausted)?;
        state.next_work_sequence = sequence.get();
        state.active.insert(slot.clone(), sequence);
        Ok(RuntimeServingSlotWorkPermitV2 {
            identity: Some(RuntimeServingSlotWorkPermitIdentityV2 {
                slot,
                coordinator_generation: epoch.coordinator_generation(),
                process_instance_id: epoch.process_instance_id().clone(),
                route_set_sequence,
                work_sequence: sequence,
            }),
            supervisor: Arc::downgrade(&self.state),
        })
    }

    pub(super) fn complete(
        &mut self,
        epoch: &RuntimeRouteSetEpochV2,
        mut permit: RuntimeServingSlotWorkPermitV2,
    ) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        let Some(supervisor) = permit.supervisor.upgrade() else {
            return Err(RuntimeServingSlotWorkErrorV2::StalePermit);
        };
        if !Arc::ptr_eq(&self.state, &supervisor) {
            return Err(RuntimeServingSlotWorkErrorV2::StalePermit);
        }
        let identity = permit
            .identity
            .as_ref()
            .ok_or(RuntimeServingSlotWorkErrorV2::StalePermit)?;
        if identity.coordinator_generation != epoch.coordinator_generation()
            || &identity.process_instance_id != epoch.process_instance_id()
        {
            return Err(RuntimeServingSlotWorkErrorV2::StalePermit);
        }
        {
            let mut state = lock_state(&self.state);
            if state.sealed {
                return Err(RuntimeServingSlotWorkErrorV2::SupervisorSealed);
            }
            if state.active.get(&identity.slot) != Some(&identity.work_sequence) {
                return Err(RuntimeServingSlotWorkErrorV2::StalePermit);
            }
            state.active.remove(&identity.slot);
        }
        permit.identity.take();
        Ok(())
    }
}

fn lock_state(
    state: &Mutex<RuntimeServingSlotWorkStateV2>,
) -> MutexGuard<'_, RuntimeServingSlotWorkStateV2> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
