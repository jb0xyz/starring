use std::collections::HashMap;
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use automation_runtime_convergence::{
    FencingToken, ProcessInstanceId, RuntimeGeneration, RuntimeProcessIdentityV1,
};

use crate::{ExactServingRouteV1, ServingSlotKeyV1, ServingSlotRegistryError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServingSlotRegistryConfigV1 {
    pub max_slots: NonZeroU32,
    pub max_active_interactions_per_slot: NonZeroU32,
    pub max_retired_routes_per_slot: NonZeroU32,
}

impl Default for ServingSlotRegistryConfigV1 {
    fn default() -> Self {
        Self {
            max_slots: NonZeroU32::new(4096).expect("default slot limit is non-zero"),
            max_active_interactions_per_slot: NonZeroU32::new(1024)
                .expect("default active interaction limit is non-zero"),
            max_retired_routes_per_slot: NonZeroU32::new(8)
                .expect("default retired route limit is non-zero"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotLifecycleV1 {
    Staged,
    Serving,
    Draining,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotInstallOutcomeV1 {
    Staged,
    AlreadyStaged,
    AlreadyServing,
    AlreadyDraining,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotInstallReceiptV1 {
    pub outcome: SlotInstallOutcomeV1,
    pub token: SlotMutationTokenV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotActivationOutcomeV1 {
    Activated,
    AlreadyServing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotDrainOutcomeV1 {
    DrainStarted { active_interactions: u32 },
    AlreadyDraining { active_interactions: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotRemovalOutcomeV1 {
    RemovedStaged,
    RemovedDraining,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotDrainObservationV1 {
    pub active_interactions: u32,
    pub drained: bool,
}

#[derive(Clone)]
pub struct SlotMutationTokenV1 {
    registry: Weak<RegistryInner>,
    key: ServingSlotKeyV1,
    identity: RuntimeProcessIdentityV1,
    fencing_token: FencingToken,
    incarnation: NonZeroU64,
}

impl fmt::Debug for SlotMutationTokenV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SlotMutationTokenV1")
            .field("key", &self.key)
            .field("identity", &self.identity)
            .field("fencing_token", &self.fencing_token)
            .field("incarnation", &self.incarnation)
            .finish()
    }
}

impl PartialEq for SlotMutationTokenV1 {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.registry, &other.registry)
            && self.key == other.key
            && self.identity == other.identity
            && self.fencing_token == other.fencing_token
            && self.incarnation == other.incarnation
    }
}

impl Eq for SlotMutationTokenV1 {}

impl SlotMutationTokenV1 {
    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.key
    }

    pub fn identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.identity
    }

    pub fn runtime_generation(&self) -> RuntimeGeneration {
        self.identity.runtime_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.identity.process_instance_id
    }

    pub fn fencing_token(&self) -> FencingToken {
        self.fencing_token
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServingSlotSnapshotV1 {
    token: SlotMutationTokenV1,
}

impl ServingSlotSnapshotV1 {
    pub fn identity(&self) -> &RuntimeProcessIdentityV1 {
        self.token.identity()
    }

    pub fn token(&self) -> &SlotMutationTokenV1 {
        &self.token
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRouteStatusV1 {
    pub lifecycle: SlotLifecycleV1,
    pub active_interactions: u32,
    pub token: SlotMutationTokenV1,
}

pub struct AdmittedInteractionV1 {
    route: ExactServingRouteV1,
    snapshot: ServingSlotSnapshotV1,
    guard: ActiveInteractionGuardV1,
}

impl AdmittedInteractionV1 {
    pub fn snapshot(&self) -> &ServingSlotSnapshotV1 {
        &self.snapshot
    }

    pub fn route(&self) -> &ExactServingRouteV1 {
        &self.route
    }

    pub fn token(&self) -> &SlotMutationTokenV1 {
        self.snapshot.token()
    }

    pub fn active_guard(&self) -> &ActiveInteractionGuardV1 {
        &self.guard
    }
}

pub struct ActiveInteractionGuardV1 {
    registry: Weak<RegistryInner>,
    key: ServingSlotKeyV1,
    incarnation: NonZeroU64,
}

impl Drop for ActiveInteractionGuardV1 {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let mut state = match registry.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(slot) = state.slots.get_mut(&self.key) else {
            return;
        };
        if let Some(route) = slot.current.as_mut() {
            if route.incarnation == self.incarnation {
                route.active_interactions = route.active_interactions.saturating_sub(1);
                return;
            }
        }
        if let Some(route) = slot.retired.get_mut(&self.incarnation) {
            route.active_interactions = route.active_interactions.saturating_sub(1);
        }
    }
}

#[derive(Clone)]
pub struct ServingSlotRegistryV1 {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    config: ServingSlotRegistryConfigV1,
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    slots: HashMap<ServingSlotKeyV1, SlotCell>,
    next_incarnation: u64,
}

#[derive(Default)]
struct SlotCell {
    high_water: Option<FenceHighWater>,
    current: Option<RouteRecord>,
    staged: Option<RouteRecord>,
    retired: HashMap<NonZeroU64, RouteRecord>,
}

struct FenceHighWater {
    generation: RuntimeGeneration,
    fencing_token: FencingToken,
    identity: RuntimeProcessIdentityV1,
}

struct RouteRecord {
    route: ExactServingRouteV1,
    fencing_token: FencingToken,
    incarnation: NonZeroU64,
    lifecycle: SlotLifecycleV1,
    active_interactions: u32,
}

impl RouteRecord {
    fn mutation_token(
        &self,
        registry: Weak<RegistryInner>,
        key: &ServingSlotKeyV1,
    ) -> SlotMutationTokenV1 {
        SlotMutationTokenV1 {
            registry,
            key: key.clone(),
            identity: self.route.identity().clone(),
            fencing_token: self.fencing_token,
            incarnation: self.incarnation,
        }
    }

    fn matches(&self, token: &SlotMutationTokenV1) -> bool {
        self.incarnation == token.incarnation
            && self.fencing_token == token.fencing_token
            && self.route.identity() == &token.identity
    }
}

impl ServingSlotRegistryV1 {
    pub fn new(config: ServingSlotRegistryConfigV1) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                config,
                state: Mutex::new(RegistryState::default()),
            }),
        }
    }

    pub fn install(
        &self,
        key: ServingSlotKeyV1,
        route: ExactServingRouteV1,
        fencing_token: FencingToken,
    ) -> Result<SlotInstallReceiptV1, ServingSlotRegistryError> {
        if route.slot_key() != key {
            return Err(ServingSlotRegistryError::TargetSlotMismatch);
        }
        let mut state = self.lock_state()?;
        if !state.slots.contains_key(&key)
            && state.slots.len() >= self.inner.config.max_slots.get() as usize
        {
            return Err(ServingSlotRegistryError::SlotCapacityExceeded);
        }
        if let Some(existing) = state.slots.get(&key).and_then(|slot| {
            find_exact_installed(slot, route.identity(), fencing_token).map(|(record, outcome)| {
                (
                    record.mutation_token(Arc::downgrade(&self.inner), &key),
                    outcome,
                )
            })
        }) {
            ensure_high_water(state.slots.get(&key), &existing.0)?;
            return Ok(SlotInstallReceiptV1 {
                outcome: existing.1,
                token: existing.0,
            });
        }
        validate_new_fence(state.slots.get(&key), route.identity(), fencing_token)?;
        let incarnation = next_incarnation(&mut state)?;
        let slot = state.slots.entry(key.clone()).or_default();
        slot.high_water = Some(FenceHighWater {
            generation: route.identity().runtime_generation,
            fencing_token,
            identity: route.identity().clone(),
        });
        let record = RouteRecord {
            route,
            fencing_token,
            incarnation,
            lifecycle: SlotLifecycleV1::Staged,
            active_interactions: 0,
        };
        let token = record.mutation_token(Arc::downgrade(&self.inner), &key);
        slot.staged = Some(record);
        Ok(SlotInstallReceiptV1 {
            outcome: SlotInstallOutcomeV1::Staged,
            token,
        })
    }

    pub fn activate(
        &self,
        token: &SlotMutationTokenV1,
        expected_identity: &RuntimeProcessIdentityV1,
    ) -> Result<SlotActivationOutcomeV1, ServingSlotRegistryError> {
        if token.identity() != expected_identity {
            return Err(ServingSlotRegistryError::ActivationTargetMismatch);
        }
        self.ensure_registry_token(token)?;
        let mut state = self.lock_state()?;
        let max_retired = self.inner.config.max_retired_routes_per_slot.get() as usize;
        let slot = state
            .slots
            .get_mut(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        if let Some(current) = &slot.current {
            if current.matches(token) && current.lifecycle == SlotLifecycleV1::Serving {
                ensure_high_water(Some(slot), token)?;
                return Ok(SlotActivationOutcomeV1::AlreadyServing);
            }
        }
        let staged_matches = slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(token));
        if !staged_matches {
            return Err(ServingSlotRegistryError::StaleMutationToken);
        }
        slot.retired
            .retain(|_, route| route.active_interactions > 0);
        let retiring_active = slot
            .current
            .as_ref()
            .is_some_and(|route| route.active_interactions > 0);
        if retiring_active && slot.retired.len() >= max_retired {
            return Err(ServingSlotRegistryError::RetiredRouteCapacityExceeded);
        }
        if let Some(mut current) = slot.current.take() {
            current.lifecycle = SlotLifecycleV1::Draining;
            if current.active_interactions > 0 {
                slot.retired.insert(current.incarnation, current);
            }
        }
        let mut staged = slot
            .staged
            .take()
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        staged.lifecycle = SlotLifecycleV1::Serving;
        slot.current = Some(staged);
        Ok(SlotActivationOutcomeV1::Activated)
    }

    pub fn begin_drain(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<SlotDrainOutcomeV1, ServingSlotRegistryError> {
        self.begin_drain_with_authority(token, token)
    }

    pub fn begin_drain_with_authority(
        &self,
        authority: &SlotMutationTokenV1,
        target: &SlotMutationTokenV1,
    ) -> Result<SlotDrainOutcomeV1, ServingSlotRegistryError> {
        self.ensure_registry_token(authority)?;
        self.ensure_registry_token(target)?;
        if authority.key() != target.key() {
            return Err(ServingSlotRegistryError::StaleMutationToken);
        }
        let mut state = self.lock_state()?;
        let slot = state
            .slots
            .get_mut(target.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        if authority == target {
            if let Some(retired) = slot.retired.get(&target.incarnation) {
                if retired.matches(target) {
                    return Ok(SlotDrainOutcomeV1::AlreadyDraining {
                        active_interactions: retired.active_interactions,
                    });
                }
            }
        }
        ensure_high_water(Some(slot), authority)?;
        let current_matches = slot
            .current
            .as_ref()
            .is_some_and(|current| current.matches(target));
        if current_matches {
            let current = slot
                .current
                .as_mut()
                .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
            return match current.lifecycle {
                SlotLifecycleV1::Serving => {
                    current.lifecycle = SlotLifecycleV1::Draining;
                    Ok(SlotDrainOutcomeV1::DrainStarted {
                        active_interactions: current.active_interactions,
                    })
                }
                SlotLifecycleV1::Draining => Ok(SlotDrainOutcomeV1::AlreadyDraining {
                    active_interactions: current.active_interactions,
                }),
                SlotLifecycleV1::Staged => Err(ServingSlotRegistryError::NotServing),
            };
        }
        if let Some(retired) = slot.retired.get(&target.incarnation) {
            if retired.matches(target) {
                return Ok(SlotDrainOutcomeV1::AlreadyDraining {
                    active_interactions: retired.active_interactions,
                });
            }
        }
        Err(ServingSlotRegistryError::StaleMutationToken)
    }

    pub fn observe_drain(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<SlotDrainObservationV1, ServingSlotRegistryError> {
        self.ensure_registry_token(token)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        let route = find_record(slot, token).ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        if route.lifecycle != SlotLifecycleV1::Draining {
            return Err(ServingSlotRegistryError::NotDraining);
        }
        Ok(SlotDrainObservationV1 {
            active_interactions: route.active_interactions,
            drained: route.active_interactions == 0,
        })
    }

    pub fn remove(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<SlotRemovalOutcomeV1, ServingSlotRegistryError> {
        self.ensure_registry_token(token)?;
        let mut state = self.lock_state()?;
        let slot = state
            .slots
            .get_mut(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        if slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(token))
        {
            ensure_high_water(Some(slot), token)?;
            slot.staged = None;
            return Ok(SlotRemovalOutcomeV1::RemovedStaged);
        }
        if slot
            .current
            .as_ref()
            .is_some_and(|current| current.matches(token))
        {
            ensure_high_water(Some(slot), token)?;
            let current = slot
                .current
                .as_ref()
                .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
            validate_removable(current)?;
            slot.current = None;
            return Ok(SlotRemovalOutcomeV1::RemovedDraining);
        }
        if let Some(retired) = slot.retired.get(&token.incarnation) {
            if retired.matches(token) {
                validate_removable(retired)?;
                slot.retired.remove(&token.incarnation);
                return Ok(SlotRemovalOutcomeV1::RemovedDraining);
            }
        }
        Err(ServingSlotRegistryError::StaleMutationToken)
    }

    pub fn remove_with_authority(
        &self,
        authority: &SlotMutationTokenV1,
        target: &SlotMutationTokenV1,
    ) -> Result<SlotRemovalOutcomeV1, ServingSlotRegistryError> {
        self.ensure_registry_token(authority)?;
        self.ensure_registry_token(target)?;
        if authority.key() != target.key() {
            return Err(ServingSlotRegistryError::StaleMutationToken);
        }
        let mut state = self.lock_state()?;
        let slot = state
            .slots
            .get_mut(target.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        ensure_high_water(Some(slot), authority)?;
        if slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(target))
        {
            slot.staged = None;
            return Ok(SlotRemovalOutcomeV1::RemovedStaged);
        }
        if slot
            .current
            .as_ref()
            .is_some_and(|current| current.matches(target))
        {
            let current = slot
                .current
                .as_ref()
                .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
            validate_removable(current)?;
            slot.current = None;
            return Ok(SlotRemovalOutcomeV1::RemovedDraining);
        }
        if let Some(retired) = slot.retired.get(&target.incarnation) {
            if retired.matches(target) {
                validate_removable(retired)?;
                slot.retired.remove(&target.incarnation);
                return Ok(SlotRemovalOutcomeV1::RemovedDraining);
            }
        }
        Err(ServingSlotRegistryError::StaleMutationToken)
    }

    pub fn serving_snapshot(
        &self,
        key: &ServingSlotKeyV1,
    ) -> Result<Option<ServingSlotSnapshotV1>, ServingSlotRegistryError> {
        let state = self.lock_state()?;
        let Some(current) = state.slots.get(key).and_then(|slot| slot.current.as_ref()) else {
            return Ok(None);
        };
        if current.lifecycle != SlotLifecycleV1::Serving {
            return Ok(None);
        }
        Ok(Some(ServingSlotSnapshotV1 {
            token: current.mutation_token(Arc::downgrade(&self.inner), key),
        }))
    }

    pub fn admit(
        &self,
        key: &ServingSlotKeyV1,
    ) -> Result<AdmittedInteractionV1, ServingSlotRegistryError> {
        let mut state = self.lock_state()?;
        let slot = state
            .slots
            .get_mut(key)
            .ok_or(ServingSlotRegistryError::NotServing)?;
        let current = slot
            .current
            .as_ref()
            .ok_or(ServingSlotRegistryError::NotServing)?;
        if current.lifecycle != SlotLifecycleV1::Serving {
            return Err(ServingSlotRegistryError::NotServing);
        }
        let total_active = u64::from(current.active_interactions)
            + slot
                .retired
                .values()
                .map(|route| u64::from(route.active_interactions))
                .sum::<u64>();
        if total_active >= u64::from(self.inner.config.max_active_interactions_per_slot.get()) {
            return Err(ServingSlotRegistryError::ActiveInteractionCapacityExceeded);
        }
        let current = slot
            .current
            .as_mut()
            .ok_or(ServingSlotRegistryError::NotServing)?;
        current.active_interactions += 1;
        let route = current.route.clone();
        let snapshot = ServingSlotSnapshotV1 {
            token: current.mutation_token(Arc::downgrade(&self.inner), key),
        };
        let guard = ActiveInteractionGuardV1 {
            registry: Arc::downgrade(&self.inner),
            key: key.clone(),
            incarnation: current.incarnation,
        };
        Ok(AdmittedInteractionV1 {
            route,
            snapshot,
            guard,
        })
    }

    pub fn route_status(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<SlotRouteStatusV1, ServingSlotRegistryError> {
        self.ensure_registry_token(token)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        let route = find_record(slot, token).ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        Ok(SlotRouteStatusV1 {
            lifecycle: route.lifecycle,
            active_interactions: route.active_interactions,
            token: route.mutation_token(Arc::downgrade(&self.inner), token.key()),
        })
    }

    fn ensure_registry_token(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<(), ServingSlotRegistryError> {
        if Weak::ptr_eq(&token.registry, &Arc::downgrade(&self.inner)) {
            Ok(())
        } else {
            Err(ServingSlotRegistryError::StaleMutationToken)
        }
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, RegistryState>, ServingSlotRegistryError> {
        self.inner
            .state
            .lock()
            .map_err(|_| ServingSlotRegistryError::RegistryPoisoned)
    }
}

fn find_exact_installed<'a>(
    slot: &'a SlotCell,
    identity: &RuntimeProcessIdentityV1,
    fencing_token: FencingToken,
) -> Option<(&'a RouteRecord, SlotInstallOutcomeV1)> {
    if let Some(staged) = &slot.staged {
        if staged.route.identity() == identity && staged.fencing_token == fencing_token {
            return Some((staged, SlotInstallOutcomeV1::AlreadyStaged));
        }
    }
    if let Some(current) = &slot.current {
        if current.route.identity() == identity && current.fencing_token == fencing_token {
            let outcome = match current.lifecycle {
                SlotLifecycleV1::Serving => SlotInstallOutcomeV1::AlreadyServing,
                SlotLifecycleV1::Draining => SlotInstallOutcomeV1::AlreadyDraining,
                SlotLifecycleV1::Staged => SlotInstallOutcomeV1::AlreadyStaged,
            };
            return Some((current, outcome));
        }
    }
    None
}

fn validate_new_fence(
    slot: Option<&SlotCell>,
    identity: &RuntimeProcessIdentityV1,
    fencing_token: FencingToken,
) -> Result<(), ServingSlotRegistryError> {
    let Some(high_water) = slot.and_then(|slot| slot.high_water.as_ref()) else {
        return Ok(());
    };
    if identity.runtime_generation < high_water.generation {
        return Err(ServingSlotRegistryError::StaleRuntimeGeneration {
            minimum: high_water.generation,
            actual: identity.runtime_generation,
        });
    }
    if identity.runtime_generation > high_water.generation {
        return Ok(());
    }
    if identity != &high_water.identity {
        return Err(ServingSlotRegistryError::RuntimeGenerationIdentityConflict);
    }
    if fencing_token <= high_water.fencing_token {
        return Err(ServingSlotRegistryError::StaleFencingToken {
            minimum: high_water.fencing_token,
            actual: fencing_token,
        });
    }
    Ok(())
}

fn next_incarnation(state: &mut RegistryState) -> Result<NonZeroU64, ServingSlotRegistryError> {
    state.next_incarnation = state
        .next_incarnation
        .checked_add(1)
        .ok_or(ServingSlotRegistryError::IncarnationExhausted)?;
    NonZeroU64::new(state.next_incarnation).ok_or(ServingSlotRegistryError::IncarnationExhausted)
}

fn ensure_high_water(
    slot: Option<&SlotCell>,
    token: &SlotMutationTokenV1,
) -> Result<(), ServingSlotRegistryError> {
    let Some(high_water) = slot.and_then(|slot| slot.high_water.as_ref()) else {
        return Err(ServingSlotRegistryError::StaleMutationToken);
    };
    if high_water.identity == token.identity && high_water.fencing_token == token.fencing_token {
        Ok(())
    } else {
        Err(ServingSlotRegistryError::StaleMutationToken)
    }
}

fn find_record<'a>(slot: &'a SlotCell, token: &SlotMutationTokenV1) -> Option<&'a RouteRecord> {
    if let Some(current) = &slot.current {
        if current.matches(token) {
            return Some(current);
        }
    }
    if let Some(staged) = &slot.staged {
        if staged.matches(token) {
            return Some(staged);
        }
    }
    slot.retired
        .get(&token.incarnation)
        .filter(|retired| retired.matches(token))
}

fn validate_removable(route: &RouteRecord) -> Result<(), ServingSlotRegistryError> {
    if route.lifecycle != SlotLifecycleV1::Draining {
        return Err(ServingSlotRegistryError::NotDraining);
    }
    if route.active_interactions > 0 {
        return Err(ServingSlotRegistryError::ActiveInteractionsRemain {
            active: route.active_interactions,
        });
    }
    Ok(())
}
