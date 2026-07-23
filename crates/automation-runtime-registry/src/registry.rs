use std::collections::HashMap;
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use automation_runtime_convergence::{
    FencingToken, ProcessInstanceId, RuntimeGeneration, RuntimeProcessIdentityV1,
};

use crate::v2_recovery::RegistryRecoveryObservationPartsV2;
use crate::{
    ExactServingRouteV1, RegistryGlobalObservationSequenceV2, RegistryRecoveryObservationV2,
    ServingSlotKeyV1, ServingSlotRegistryError, SlotAdmissionStateV2, SlotAtomicObservationV2,
};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRouteWitnessV1 {
    pub identity: RuntimeProcessIdentityV1,
    pub fencing_token: FencingToken,
    pub incarnation: NonZeroU64,
    pub lifecycle: SlotLifecycleV1,
}

pub struct SlotActivationRecordV2 {
    outcome: SlotActivationOutcomeV1,
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    observation: SlotAtomicObservationV2,
}

impl SlotActivationRecordV2 {
    pub const fn outcome(&self) -> SlotActivationOutcomeV1 {
        self.outcome
    }

    pub fn route(&self) -> &SlotRouteWitnessV1 {
        &self.route
    }

    pub const fn activation_sequence(&self) -> NonZeroU64 {
        self.activation_sequence
    }

    pub fn observation(&self) -> &SlotAtomicObservationV2 {
        &self.observation
    }
}

pub struct AdmittedInteractionV1 {
    route: ExactServingRouteV1,
    snapshot: ServingSlotSnapshotV1,
    guard: ActiveInteractionGuardV1,
}

pub struct AdmittedInteractionV2 {
    route: ExactServingRouteV1,
    guard: ActiveInteractionGuardV1,
    observation: SlotAtomicObservationV2,
}

impl AdmittedInteractionV2 {
    pub fn route(&self) -> &ExactServingRouteV1 {
        &self.route
    }

    pub fn active_guard(&self) -> &ActiveInteractionGuardV1 {
        &self.guard
    }

    pub fn observation(&self) -> &SlotAtomicObservationV2 {
        &self.observation
    }
}

pub struct SlotDrainClaimSealV2 {
    registry: Weak<RegistryInner>,
    key: ServingSlotKeyV1,
    seal_key: crate::SlotSealKeyV2,
    seal_generation: NonZeroU64,
    route: Option<SlotRouteWitnessV1>,
}

struct AdmittedInteractionPartsV2 {
    route: ExactServingRouteV1,
    snapshot: ServingSlotSnapshotV1,
    guard: ActiveInteractionGuardV1,
    observation: SlotAtomicObservationV2,
}

impl SlotDrainClaimSealV2 {
    pub fn key(&self) -> &ServingSlotKeyV1 {
        &self.key
    }

    pub const fn seal_key(&self) -> crate::SlotSealKeyV2 {
        self.seal_key
    }

    pub const fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn route(&self) -> Option<&SlotRouteWitnessV1> {
        self.route.as_ref()
    }
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
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let Some(slot) = slots.get_mut(&self.key) else {
            return;
        };
        let current_matches = slot
            .current
            .as_ref()
            .is_some_and(|route| route.incarnation == self.incarnation);
        if current_matches {
            if advance_observation_or_close(observation, slot) {
                if let Some(route) = slot.current.as_mut() {
                    route.active_interactions = route.active_interactions.saturating_sub(1);
                }
            }
            return;
        }
        if slot.retired.contains_key(&self.incarnation)
            && advance_observation_or_close(observation, slot)
        {
            if let Some(route) = slot.retired.get_mut(&self.incarnation) {
                route.active_interactions = route.active_interactions.saturating_sub(1);
            }
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

pub struct RegistryEmptyRecoveryCursorV2 {
    registry: Weak<RegistryInner>,
    expected_sequence: RegistryGlobalObservationSequenceV2,
}

impl fmt::Debug for RegistryEmptyRecoveryCursorV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RegistryEmptyRecoveryCursorV2(<redacted>)")
    }
}

pub struct RegistryRecoveryObservationGuardV2<'a> {
    _state: MutexGuard<'a, RegistryState>,
    observation: RegistryRecoveryObservationV2,
    registry: Weak<RegistryInner>,
}

impl RegistryRecoveryObservationGuardV2<'_> {
    pub const fn observation(&self) -> RegistryRecoveryObservationV2 {
        self.observation
    }

    pub fn into_empty_cursor(
        self,
    ) -> Result<RegistryEmptyRecoveryCursorV2, ServingSlotRegistryError> {
        if !self.observation.is_recovery_empty() {
            return Err(ServingSlotRegistryError::RegistryRecoveryNotEmpty);
        }
        Ok(RegistryEmptyRecoveryCursorV2 {
            registry: self.registry,
            expected_sequence: self.observation.observation_sequence(),
        })
    }
}

#[derive(Default)]
struct RegistryState {
    slots: HashMap<ServingSlotKeyV1, SlotCell>,
    next_incarnation: u64,
    observation: RegistryObservationState,
}

struct RegistryObservationState {
    sequence: RegistryGlobalObservationSequenceV2,
    failed_closed: bool,
}

impl Default for RegistryObservationState {
    fn default() -> Self {
        Self {
            sequence: RegistryGlobalObservationSequenceV2::new(NonZeroU64::MIN),
            failed_closed: false,
        }
    }
}

struct SlotCell {
    high_water: Option<FenceHighWater>,
    current: Option<RouteRecord>,
    staged: Option<RouteRecord>,
    retired: HashMap<NonZeroU64, RouteRecord>,
    admission_generation: NonZeroU64,
    observation_sequence: NonZeroU64,
    next_activation_sequence: u64,
    next_seal_generation: u64,
    seal: Option<SlotSealState>,
    failed_closed: bool,
}

impl Default for SlotCell {
    fn default() -> Self {
        Self {
            high_water: None,
            current: None,
            staged: None,
            retired: HashMap::new(),
            admission_generation: NonZeroU64::MIN,
            observation_sequence: NonZeroU64::MIN,
            next_activation_sequence: 0,
            next_seal_generation: 0,
            seal: None,
            failed_closed: false,
        }
    }
}

struct SlotSealState {
    seal_key: crate::SlotSealKeyV2,
    seal_generation: NonZeroU64,
    route: Option<SlotRouteWitnessV1>,
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
    activation_sequence: Option<NonZeroU64>,
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
        let slot_is_new = !state.slots.contains_key(&key);
        if slot_is_new && state.slots.len() >= self.inner.config.max_slots.get() as usize {
            return Err(ServingSlotRegistryError::SlotCapacityExceeded);
        }
        if let Some(slot) = state.slots.get(&key) {
            ensure_slot_open(slot)?;
            ensure_slot_unsealed(slot)?;
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
        let incarnation = next_incarnation(&state)?;
        if slot_is_new {
            advance_registry_observation(&mut state.observation)?;
        } else {
            let RegistryState {
                slots, observation, ..
            } = &mut *state;
            let slot = slots
                .get_mut(&key)
                .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
            advance_slot_mutation(observation, slot)?;
        }
        state.next_incarnation = incarnation.get();
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
            activation_sequence: None,
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
        self.activate_with_sequence_v2(token, expected_identity)
            .map(|receipt| receipt.outcome())
    }

    pub fn activate_with_sequence_v2(
        &self,
        token: &SlotMutationTokenV1,
        expected_identity: &RuntimeProcessIdentityV1,
    ) -> Result<SlotActivationRecordV2, ServingSlotRegistryError> {
        if token.identity() != expected_identity {
            return Err(ServingSlotRegistryError::ActivationTargetMismatch);
        }
        self.ensure_registry_token(token)?;
        let mut state = self.lock_state()?;
        let max_retired = self.inner.config.max_retired_routes_per_slot.get() as usize;
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
        if let Some(current) = &slot.current {
            if current.matches(token) && current.lifecycle == SlotLifecycleV1::Serving {
                ensure_high_water(Some(slot), token)?;
                return activation_record_v2(
                    slot,
                    current,
                    SlotActivationOutcomeV1::AlreadyServing,
                );
            }
        }
        let staged_matches = slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(token));
        if !staged_matches {
            return Err(ServingSlotRegistryError::StaleMutationToken);
        }
        let retained_route_count = slot
            .retired
            .values()
            .filter(|route| route.active_interactions > 0)
            .count();
        let retiring_active = slot
            .current
            .as_ref()
            .is_some_and(|route| route.active_interactions > 0);
        if retiring_active && retained_route_count >= max_retired {
            return Err(ServingSlotRegistryError::RetiredRouteCapacityExceeded);
        }
        let activation_sequence = advance_slot_activation(observation, slot)?;
        slot.retired
            .retain(|_, route| route.active_interactions > 0);
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
        staged.activation_sequence = Some(activation_sequence);
        slot.current = Some(staged);
        let current = slot
            .current
            .as_ref()
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        activation_record_v2(slot, current, SlotActivationOutcomeV1::Activated)
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
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(target.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
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
            let lifecycle = slot
                .current
                .as_ref()
                .ok_or(ServingSlotRegistryError::StaleMutationToken)?
                .lifecycle;
            return match lifecycle {
                SlotLifecycleV1::Serving => {
                    advance_slot_mutation(observation, slot)?;
                    let current = slot
                        .current
                        .as_mut()
                        .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
                    current.lifecycle = SlotLifecycleV1::Draining;
                    Ok(SlotDrainOutcomeV1::DrainStarted {
                        active_interactions: current.active_interactions,
                    })
                }
                SlotLifecycleV1::Draining => {
                    let active_interactions = slot
                        .current
                        .as_ref()
                        .ok_or(ServingSlotRegistryError::StaleMutationToken)?
                        .active_interactions;
                    Ok(SlotDrainOutcomeV1::AlreadyDraining {
                        active_interactions,
                    })
                }
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
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
        if slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(token))
        {
            ensure_high_water(Some(slot), token)?;
            advance_slot_mutation(observation, slot)?;
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
            advance_slot_mutation(observation, slot)?;
            slot.current = None;
            return Ok(SlotRemovalOutcomeV1::RemovedDraining);
        }
        if let Some(retired) = slot.retired.get(&token.incarnation) {
            if retired.matches(token) {
                validate_removable(retired)?;
                advance_slot_mutation(observation, slot)?;
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
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(target.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
        ensure_high_water(Some(slot), authority)?;
        if slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(target))
        {
            advance_slot_mutation(observation, slot)?;
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
            advance_slot_mutation(observation, slot)?;
            slot.current = None;
            return Ok(SlotRemovalOutcomeV1::RemovedDraining);
        }
        if let Some(retired) = slot.retired.get(&target.incarnation) {
            if retired.matches(target) {
                validate_removable(retired)?;
                advance_slot_mutation(observation, slot)?;
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
        let Some(slot) = state.slots.get(key) else {
            return Ok(None);
        };
        ensure_slot_open(slot)?;
        if slot.seal.is_some() {
            return Ok(None);
        }
        let Some(current) = slot.current.as_ref() else {
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
        self.admit_internal_v2(key, None)
            .map(|parts| AdmittedInteractionV1 {
                route: parts.route,
                snapshot: parts.snapshot,
                guard: parts.guard,
            })
    }

    pub fn admit_at_generation_v2(
        &self,
        key: &ServingSlotKeyV1,
        expected_admission_generation: NonZeroU64,
    ) -> Result<AdmittedInteractionV2, ServingSlotRegistryError> {
        self.admit_internal_v2(key, Some(expected_admission_generation))
            .map(|parts| AdmittedInteractionV2 {
                route: parts.route,
                guard: parts.guard,
                observation: parts.observation,
            })
    }

    fn admit_internal_v2(
        &self,
        key: &ServingSlotKeyV1,
        expected_admission_generation: Option<NonZeroU64>,
    ) -> Result<AdmittedInteractionPartsV2, ServingSlotRegistryError> {
        let mut state = self.lock_state()?;
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(key)
            .ok_or(ServingSlotRegistryError::NotServing)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
        if let Some(expected) = expected_admission_generation {
            if slot.admission_generation != expected {
                return Err(ServingSlotRegistryError::AdmissionGenerationMismatch {
                    expected,
                    actual: slot.admission_generation,
                });
            }
        }
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
        advance_slot_observation(observation, slot)?;
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
        Ok(AdmittedInteractionPartsV2 {
            route,
            snapshot,
            guard,
            observation: atomic_observation_v2(slot)?,
        })
    }

    pub fn atomic_observation_v2(
        &self,
        key: &ServingSlotKeyV1,
    ) -> Result<Option<SlotAtomicObservationV2>, ServingSlotRegistryError> {
        let state = self.lock_state()?;
        state.slots.get(key).map(atomic_observation_v2).transpose()
    }

    pub fn recovery_observation_v2(
        &self,
    ) -> Result<RegistryRecoveryObservationV2, ServingSlotRegistryError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ServingSlotRegistryError::RegistryPoisoned)?;
        registry_recovery_observation_v2(&state)
    }

    pub fn recovery_observation_guard_v2(
        &self,
    ) -> Result<RegistryRecoveryObservationGuardV2<'_>, ServingSlotRegistryError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ServingSlotRegistryError::RegistryPoisoned)?;
        let observation = registry_recovery_observation_v2(&state)?;
        Ok(RegistryRecoveryObservationGuardV2 {
            _state: state,
            observation,
            registry: Arc::downgrade(&self.inner),
        })
    }

    pub fn revalidate_empty_recovery_cursor_v2(
        &self,
        cursor: &RegistryEmptyRecoveryCursorV2,
    ) -> Result<RegistryRecoveryObservationV2, ServingSlotRegistryError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ServingSlotRegistryError::RegistryPoisoned)?;
        if !Weak::ptr_eq(&cursor.registry, &Arc::downgrade(&self.inner)) {
            return Err(ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor);
        }
        let observation = registry_recovery_observation_v2(&state)?;
        if observation.observation_sequence() != cursor.expected_sequence {
            return Err(ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor);
        }
        if !observation.is_recovery_empty() {
            return Err(ServingSlotRegistryError::RegistryRecoveryNotEmpty);
        }
        Ok(observation)
    }

    pub fn seal_drain_claim_v2(
        &self,
        key: &ServingSlotKeyV1,
        seal_key: crate::SlotSealKeyV2,
        expected: Option<&SlotAtomicObservationV2>,
    ) -> Result<(SlotDrainClaimSealV2, SlotAtomicObservationV2), ServingSlotRegistryError> {
        let mut state = self.lock_state()?;
        let slot_is_new = !state.slots.contains_key(key);
        if slot_is_new && state.slots.len() >= self.inner.config.max_slots.get() as usize {
            return Err(ServingSlotRegistryError::SlotCapacityExceeded);
        }
        if slot_is_new {
            if expected.is_some() {
                return Err(ServingSlotRegistryError::StaleSlotObservation);
            }
        } else {
            let slot = state
                .slots
                .get(key)
                .ok_or(ServingSlotRegistryError::StaleSlotObservation)?;
            ensure_slot_open(slot)?;
            ensure_slot_unsealed(slot)?;
            if expected != Some(&atomic_observation_v2(slot)?) {
                return Err(ServingSlotRegistryError::StaleSlotObservation);
            }
        }
        if slot_is_new {
            let mut slot = SlotCell::default();
            let seal_generation = advance_slot_seal(&mut state.observation, &mut slot, true)?;
            slot.seal = Some(SlotSealState {
                seal_key,
                seal_generation,
                route: None,
            });
            let observed = atomic_observation_v2(&slot)?;
            state.slots.insert(key.clone(), slot);
            return Ok((
                SlotDrainClaimSealV2 {
                    registry: Arc::downgrade(&self.inner),
                    key: key.clone(),
                    seal_key,
                    seal_generation,
                    route: None,
                },
                observed,
            ));
        }
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(key)
            .ok_or(ServingSlotRegistryError::StaleSlotObservation)?;
        let route = selected_route(slot).map(route_witness_v1);
        let seal_generation = advance_slot_seal(observation, slot, false)?;
        slot.seal = Some(SlotSealState {
            seal_key,
            seal_generation,
            route: route.clone(),
        });
        let capability = SlotDrainClaimSealV2 {
            registry: Arc::downgrade(&self.inner),
            key: key.clone(),
            seal_key,
            seal_generation,
            route,
        };
        Ok((capability, atomic_observation_v2(slot)?))
    }

    pub fn unseal_drain_claim_v2(
        &self,
        capability: SlotDrainClaimSealV2,
    ) -> Result<SlotAtomicObservationV2, ServingSlotRegistryError> {
        if !Weak::ptr_eq(&capability.registry, &Arc::downgrade(&self.inner)) {
            return Err(ServingSlotRegistryError::StaleSlotSeal);
        }
        let mut state = self.lock_state()?;
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(&capability.key)
            .ok_or(ServingSlotRegistryError::StaleSlotSeal)?;
        ensure_slot_open(slot)?;
        let seal_matches = slot.seal.as_ref().is_some_and(|seal| {
            seal.seal_key == capability.seal_key
                && seal.seal_generation == capability.seal_generation
                && seal.route == capability.route
        });
        if !seal_matches || selected_route(slot).map(route_witness_v1) != capability.route {
            return Err(ServingSlotRegistryError::StaleSlotSeal);
        }
        advance_slot_mutation(observation, slot)?;
        slot.seal = None;
        atomic_observation_v2(slot)
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

    pub fn route_witness(
        &self,
        token: &SlotMutationTokenV1,
    ) -> Result<SlotRouteWitnessV1, ServingSlotRegistryError> {
        self.ensure_registry_token(token)?;
        let state = self.lock_state()?;
        let slot = state
            .slots
            .get(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        let route = find_record(slot, token).ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        Ok(SlotRouteWitnessV1 {
            identity: route.route.identity().clone(),
            fencing_token: route.fencing_token,
            incarnation: route.incarnation,
            lifecycle: route.lifecycle,
        })
    }

    pub fn advance_authority(
        &self,
        token: &SlotMutationTokenV1,
        expected_identity: &RuntimeProcessIdentityV1,
        next_fencing_token: FencingToken,
    ) -> Result<SlotMutationTokenV1, ServingSlotRegistryError> {
        self.ensure_registry_token(token)?;
        let mut state = self.lock_state()?;
        let RegistryState {
            slots, observation, ..
        } = &mut *state;
        let slot = slots
            .get_mut(token.key())
            .ok_or(ServingSlotRegistryError::StaleMutationToken)?;
        ensure_slot_open(slot)?;
        ensure_slot_unsealed(slot)?;
        ensure_high_water(Some(slot), token)?;
        let staged_matches = slot
            .staged
            .as_ref()
            .is_some_and(|staged| staged.matches(token));
        let current_matches = slot
            .current
            .as_ref()
            .is_some_and(|current| current.matches(token));
        if !staged_matches && !current_matches {
            return Err(ServingSlotRegistryError::StaleMutationToken);
        }
        if token.identity() != expected_identity {
            return Err(ServingSlotRegistryError::AuthorityTargetMismatch);
        }
        let expected_fencing_token = token
            .fencing_token()
            .next()
            .map_err(|_| ServingSlotRegistryError::FencingTokenExhausted)?;
        if next_fencing_token != expected_fencing_token {
            return Err(ServingSlotRegistryError::NonSuccessorFencingToken {
                expected: expected_fencing_token,
                actual: next_fencing_token,
            });
        }
        advance_slot_mutation(observation, slot)?;
        let (route, high_water) = if staged_matches {
            match (&mut slot.staged, &mut slot.high_water) {
                (Some(staged), Some(high_water)) => (staged, high_water),
                _ => return Err(ServingSlotRegistryError::StaleMutationToken),
            }
        } else {
            match (&mut slot.current, &mut slot.high_water) {
                (Some(current), Some(high_water)) => (current, high_water),
                _ => return Err(ServingSlotRegistryError::StaleMutationToken),
            }
        };
        route.fencing_token = next_fencing_token;
        high_water.fencing_token = next_fencing_token;
        Ok(route.mutation_token(Arc::downgrade(&self.inner), token.key()))
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
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ServingSlotRegistryError::RegistryPoisoned)?;
        ensure_registry_open(&state.observation)?;
        Ok(state)
    }
}

fn ensure_slot_open(slot: &SlotCell) -> Result<(), ServingSlotRegistryError> {
    if slot.failed_closed {
        Err(ServingSlotRegistryError::SlotSequenceExhausted)
    } else {
        Ok(())
    }
}

fn ensure_slot_unsealed(slot: &SlotCell) -> Result<(), ServingSlotRegistryError> {
    if slot.seal.is_some() {
        Err(ServingSlotRegistryError::SlotSealed)
    } else {
        Ok(())
    }
}

fn successor(value: NonZeroU64) -> Option<NonZeroU64> {
    value.get().checked_add(1).and_then(NonZeroU64::new)
}

fn close_slot(slot: &mut SlotCell) -> ServingSlotRegistryError {
    slot.failed_closed = true;
    ServingSlotRegistryError::SlotSequenceExhausted
}

fn ensure_registry_open(
    observation: &RegistryObservationState,
) -> Result<(), ServingSlotRegistryError> {
    if observation.failed_closed {
        Err(ServingSlotRegistryError::RegistrySequenceExhausted)
    } else {
        Ok(())
    }
}

fn advance_registry_observation(
    observation: &mut RegistryObservationState,
) -> Result<(), ServingSlotRegistryError> {
    ensure_registry_open(observation)?;
    let Some(sequence) = successor(observation.sequence.value()) else {
        observation.failed_closed = true;
        return Err(ServingSlotRegistryError::RegistrySequenceExhausted);
    };
    observation.sequence = RegistryGlobalObservationSequenceV2::new(sequence);
    if sequence == NonZeroU64::MAX {
        observation.failed_closed = true;
        return Err(ServingSlotRegistryError::RegistrySequenceExhausted);
    }
    Ok(())
}

fn advance_slot_mutation(
    observation: &mut RegistryObservationState,
    slot: &mut SlotCell,
) -> Result<(), ServingSlotRegistryError> {
    ensure_slot_open(slot)?;
    advance_registry_observation(observation)?;
    let Some(admission_generation) = successor(slot.admission_generation) else {
        return Err(close_slot(slot));
    };
    let Some(observation_sequence) = successor(slot.observation_sequence) else {
        return Err(close_slot(slot));
    };
    slot.admission_generation = admission_generation;
    slot.observation_sequence = observation_sequence;
    Ok(())
}

fn advance_slot_activation(
    observation: &mut RegistryObservationState,
    slot: &mut SlotCell,
) -> Result<NonZeroU64, ServingSlotRegistryError> {
    ensure_slot_open(slot)?;
    advance_registry_observation(observation)?;
    let Some(admission_generation) = successor(slot.admission_generation) else {
        return Err(close_slot(slot));
    };
    let Some(observation_sequence) = successor(slot.observation_sequence) else {
        return Err(close_slot(slot));
    };
    let Some(next_activation_sequence) = slot.next_activation_sequence.checked_add(1) else {
        return Err(close_slot(slot));
    };
    let Some(activation_sequence) = NonZeroU64::new(next_activation_sequence) else {
        return Err(close_slot(slot));
    };
    slot.admission_generation = admission_generation;
    slot.observation_sequence = observation_sequence;
    slot.next_activation_sequence = next_activation_sequence;
    Ok(activation_sequence)
}

fn advance_slot_seal(
    registry_observation: &mut RegistryObservationState,
    slot: &mut SlotCell,
    materializing: bool,
) -> Result<NonZeroU64, ServingSlotRegistryError> {
    ensure_slot_open(slot)?;
    advance_registry_observation(registry_observation)?;
    let admission_generation = if materializing {
        slot.admission_generation
    } else {
        successor(slot.admission_generation).ok_or_else(|| close_slot(slot))?
    };
    let observation_sequence = if materializing {
        slot.observation_sequence
    } else {
        successor(slot.observation_sequence).ok_or_else(|| close_slot(slot))?
    };
    let Some(next_seal_generation) = slot.next_seal_generation.checked_add(1) else {
        return Err(close_slot(slot));
    };
    let Some(seal_generation) = NonZeroU64::new(next_seal_generation) else {
        return Err(close_slot(slot));
    };
    slot.admission_generation = admission_generation;
    slot.observation_sequence = observation_sequence;
    slot.next_seal_generation = next_seal_generation;
    Ok(seal_generation)
}

fn advance_slot_observation(
    observation: &mut RegistryObservationState,
    slot: &mut SlotCell,
) -> Result<(), ServingSlotRegistryError> {
    ensure_slot_open(slot)?;
    advance_registry_observation(observation)?;
    let Some(observation_sequence) = successor(slot.observation_sequence) else {
        return Err(close_slot(slot));
    };
    slot.observation_sequence = observation_sequence;
    Ok(())
}

fn advance_observation_or_close(
    observation: &mut RegistryObservationState,
    slot: &mut SlotCell,
) -> bool {
    if observation.failed_closed {
        return false;
    }
    if advance_registry_observation(observation).is_err() {
        return false;
    }
    if slot.failed_closed {
        return true;
    }
    let Some(observation_sequence) = successor(slot.observation_sequence) else {
        slot.failed_closed = true;
        return true;
    };
    slot.observation_sequence = observation_sequence;
    true
}

#[derive(Default)]
struct RegistryRecoveryCountsV2 {
    retained_empty_tombstone_count: u64,
    staged_route_count: u64,
    serving_route_count: u64,
    draining_route_count: u64,
    sealed_slot_count: u64,
    active_interaction_count: u64,
    failed_closed_slot_count: u64,
}

fn registry_recovery_observation_v2(
    state: &RegistryState,
) -> Result<RegistryRecoveryObservationV2, ServingSlotRegistryError> {
    let retained_slot_count = u64::try_from(state.slots.len())
        .map_err(|_| ServingSlotRegistryError::RegistryObservationOverflow)?;
    let mut counts = RegistryRecoveryCountsV2::default();
    for slot in state.slots.values() {
        if slot.failed_closed {
            increment_recovery_count(&mut counts.failed_closed_slot_count)?;
        }
        if let Some(seal) = &slot.seal {
            if seal.route != selected_route(slot).map(route_witness_v1) {
                return Err(ServingSlotRegistryError::RegistryObservationInvalid);
            }
            increment_recovery_count(&mut counts.sealed_slot_count)?;
        }
        if slot.current.is_none()
            && slot.staged.is_none()
            && slot.retired.is_empty()
            && slot.seal.is_none()
        {
            increment_recovery_count(&mut counts.retained_empty_tombstone_count)?;
        }
        if let Some(route) = &slot.staged {
            if route.lifecycle != SlotLifecycleV1::Staged || route.active_interactions != 0 {
                return Err(ServingSlotRegistryError::RegistryObservationInvalid);
            }
            count_recovery_route(route, &mut counts)?;
        }
        if let Some(route) = &slot.current {
            if route.lifecycle == SlotLifecycleV1::Staged {
                return Err(ServingSlotRegistryError::RegistryObservationInvalid);
            }
            count_recovery_route(route, &mut counts)?;
        }
        for route in slot.retired.values() {
            if route.lifecycle != SlotLifecycleV1::Draining {
                return Err(ServingSlotRegistryError::RegistryObservationInvalid);
            }
            count_recovery_route(route, &mut counts)?;
        }
    }
    Ok(RegistryRecoveryObservationV2::new(
        RegistryRecoveryObservationPartsV2 {
            observation_sequence: state.observation.sequence,
            retained_slot_count,
            retained_empty_tombstone_count: counts.retained_empty_tombstone_count,
            staged_route_count: counts.staged_route_count,
            serving_route_count: counts.serving_route_count,
            draining_route_count: counts.draining_route_count,
            sealed_slot_count: counts.sealed_slot_count,
            active_interaction_count: counts.active_interaction_count,
            failed_closed_slot_count: counts.failed_closed_slot_count,
            registry_failed_closed: state.observation.failed_closed,
        },
    ))
}

fn count_recovery_route(
    route: &RouteRecord,
    counts: &mut RegistryRecoveryCountsV2,
) -> Result<(), ServingSlotRegistryError> {
    let target = match route.lifecycle {
        SlotLifecycleV1::Staged => &mut counts.staged_route_count,
        SlotLifecycleV1::Serving => &mut counts.serving_route_count,
        SlotLifecycleV1::Draining => &mut counts.draining_route_count,
    };
    increment_recovery_count(target)?;
    counts.active_interaction_count = counts
        .active_interaction_count
        .checked_add(u64::from(route.active_interactions))
        .ok_or(ServingSlotRegistryError::RegistryObservationOverflow)?;
    Ok(())
}

fn increment_recovery_count(value: &mut u64) -> Result<(), ServingSlotRegistryError> {
    *value = value
        .checked_add(1)
        .ok_or(ServingSlotRegistryError::RegistryObservationOverflow)?;
    Ok(())
}

fn route_witness_v1(route: &RouteRecord) -> SlotRouteWitnessV1 {
    SlotRouteWitnessV1 {
        identity: route.route.identity().clone(),
        fencing_token: route.fencing_token,
        incarnation: route.incarnation,
        lifecycle: route.lifecycle,
    }
}

fn selected_route(slot: &SlotCell) -> Option<&RouteRecord> {
    slot.current.as_ref().or(slot.staged.as_ref())
}

fn atomic_observation_v2(
    slot: &SlotCell,
) -> Result<SlotAtomicObservationV2, ServingSlotRegistryError> {
    ensure_slot_open(slot)?;
    let route = selected_route(slot);
    let admission_state = match &slot.seal {
        Some(seal) => SlotAdmissionStateV2::DrainClaimSealed {
            seal_key: seal.seal_key,
            seal_generation: seal.seal_generation,
        },
        None => match route.map(|route| route.lifecycle) {
            Some(SlotLifecycleV1::Staged) => SlotAdmissionStateV2::Staged,
            Some(SlotLifecycleV1::Serving) => SlotAdmissionStateV2::Serving,
            Some(SlotLifecycleV1::Draining) => SlotAdmissionStateV2::Draining,
            None => SlotAdmissionStateV2::Empty,
        },
    };
    Ok(SlotAtomicObservationV2 {
        route: route.map(route_witness_v1),
        admission_state,
        active_interactions: route.map_or(0, |route| route.active_interactions),
        admission_generation: slot.admission_generation,
        observation_sequence: slot.observation_sequence,
    })
}

fn activation_record_v2(
    slot: &SlotCell,
    route: &RouteRecord,
    outcome: SlotActivationOutcomeV1,
) -> Result<SlotActivationRecordV2, ServingSlotRegistryError> {
    let activation_sequence = route
        .activation_sequence
        .ok_or(ServingSlotRegistryError::SlotSequenceExhausted)?;
    Ok(SlotActivationRecordV2 {
        outcome,
        route: route_witness_v1(route),
        activation_sequence,
        observation: atomic_observation_v2(slot)?,
    })
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

fn next_incarnation(state: &RegistryState) -> Result<NonZeroU64, ServingSlotRegistryError> {
    let next = state
        .next_incarnation
        .checked_add(1)
        .ok_or(ServingSlotRegistryError::IncarnationExhausted)?;
    NonZeroU64::new(next).ok_or(ServingSlotRegistryError::IncarnationExhausted)
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

#[cfg(test)]
mod tests {
    use super::{
        advance_observation_or_close, advance_slot_activation, advance_slot_mutation,
        atomic_observation_v2, registry_recovery_observation_v2, RegistryObservationState,
        RegistryState, ServingSlotRegistryConfigV1, ServingSlotRegistryV1, SlotCell,
    };
    use crate::{RegistryGlobalObservationSequenceV2, ServingSlotKeyV1, ServingSlotRegistryError};
    use automation_ruleset::RuleSetKey;
    use discord_model::GuildId;
    use std::num::NonZeroU64;

    #[test]
    fn slot_counter_overflow_is_terminal_and_non_mutating() {
        let mut slot = SlotCell {
            admission_generation: NonZeroU64::MAX,
            ..SlotCell::default()
        };
        let mut registry_observation = RegistryObservationState::default();
        let observation_sequence = slot.observation_sequence;
        assert_eq!(
            advance_slot_mutation(&mut registry_observation, &mut slot),
            Err(ServingSlotRegistryError::SlotSequenceExhausted)
        );
        assert_eq!(slot.admission_generation, NonZeroU64::MAX);
        assert_eq!(slot.observation_sequence, observation_sequence);
        assert_eq!(registry_observation.sequence.get(), 2);
        assert!(slot.failed_closed);
        assert_eq!(
            advance_slot_mutation(&mut registry_observation, &mut slot),
            Err(ServingSlotRegistryError::SlotSequenceExhausted)
        );
        assert_eq!(
            atomic_observation_v2(&slot),
            Err(ServingSlotRegistryError::SlotSequenceExhausted)
        );
    }

    #[test]
    fn activation_and_guard_sequence_overflow_close_the_slot() {
        let mut activation = SlotCell {
            next_activation_sequence: u64::MAX,
            ..SlotCell::default()
        };
        let admission_generation = activation.admission_generation;
        let observation_sequence = activation.observation_sequence;
        let mut activation_registry_observation = RegistryObservationState::default();
        assert_eq!(
            advance_slot_activation(&mut activation_registry_observation, &mut activation),
            Err(ServingSlotRegistryError::SlotSequenceExhausted)
        );
        assert_eq!(activation.admission_generation, admission_generation);
        assert_eq!(activation.observation_sequence, observation_sequence);

        let mut guard = SlotCell {
            observation_sequence: NonZeroU64::MAX,
            ..SlotCell::default()
        };
        let mut guard_registry_observation = RegistryObservationState::default();
        assert!(advance_observation_or_close(
            &mut guard_registry_observation,
            &mut guard
        ));
        assert!(advance_observation_or_close(
            &mut guard_registry_observation,
            &mut guard
        ));
        assert_eq!(guard_registry_observation.sequence.get(), 3);
        assert_eq!(
            advance_slot_mutation(&mut guard_registry_observation, &mut guard),
            Err(ServingSlotRegistryError::SlotSequenceExhausted)
        );
    }

    #[test]
    fn local_sequence_failure_is_visible_in_the_global_recovery_observation() {
        let mut slot = SlotCell {
            admission_generation: NonZeroU64::MAX,
            ..SlotCell::default()
        };
        let mut observation = RegistryObservationState::default();
        assert_eq!(
            advance_slot_mutation(&mut observation, &mut slot),
            Err(ServingSlotRegistryError::SlotSequenceExhausted)
        );
        let key = ServingSlotKeyV1::new(GuildId(1), RuleSetKey::parse("study").unwrap());
        let state = RegistryState {
            slots: [(key, slot)].into_iter().collect(),
            next_incarnation: 0,
            observation,
        };
        let recovery = registry_recovery_observation_v2(&state).unwrap();
        assert_eq!(recovery.observation_sequence().get(), 2);
        assert_eq!(recovery.failed_closed_slot_count(), 1);
        assert!(!recovery.is_recovery_empty());
    }

    #[test]
    fn global_sequence_terminal_value_rejects_and_freezes_the_intended_mutation() {
        let mut observation = RegistryObservationState {
            sequence: RegistryGlobalObservationSequenceV2::new(
                NonZeroU64::new(u64::MAX - 1).unwrap(),
            ),
            failed_closed: false,
        };
        let mut slot = SlotCell::default();
        let admission_generation = slot.admission_generation;
        let slot_observation_sequence = slot.observation_sequence;
        assert_eq!(
            advance_slot_mutation(&mut observation, &mut slot),
            Err(ServingSlotRegistryError::RegistrySequenceExhausted)
        );
        assert_eq!(observation.sequence.get(), u64::MAX);
        assert!(observation.failed_closed);
        assert_eq!(slot.admission_generation, admission_generation);
        assert_eq!(slot.observation_sequence, slot_observation_sequence);
        assert!(!slot.failed_closed);
        assert!(!advance_observation_or_close(&mut observation, &mut slot));

        let state = RegistryState {
            slots: Default::default(),
            next_incarnation: 0,
            observation,
        };
        let recovery = registry_recovery_observation_v2(&state).unwrap();
        assert_eq!(recovery.observation_sequence().get(), u64::MAX);
        assert!(recovery.registry_failed_closed());
        assert!(!recovery.is_recovery_empty());
    }

    #[test]
    fn empty_seal_global_terminal_failure_does_not_materialize_a_slot() {
        let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1::default());
        {
            let mut state = registry.inner.state.lock().unwrap();
            state.observation.sequence =
                RegistryGlobalObservationSequenceV2::new(NonZeroU64::new(u64::MAX - 1).unwrap());
        }
        let key = ServingSlotKeyV1::new(GuildId(1), RuleSetKey::parse("study").unwrap());
        let seal_key = crate::SlotSealKeyV2::try_from([7_u8; 16].as_slice()).unwrap();
        assert!(matches!(
            registry.seal_drain_claim_v2(&key, seal_key, None),
            Err(ServingSlotRegistryError::RegistrySequenceExhausted)
        ));
        let terminal = registry.recovery_observation_v2().unwrap();
        assert_eq!(terminal.observation_sequence().get(), u64::MAX);
        assert_eq!(terminal.retained_slot_count(), 0);
        assert!(terminal.registry_failed_closed());
        assert!(!terminal.is_recovery_empty());
        assert!(matches!(
            registry.atomic_observation_v2(&key),
            Err(ServingSlotRegistryError::RegistrySequenceExhausted)
        ));
        assert!(matches!(
            registry.serving_snapshot(&key),
            Err(ServingSlotRegistryError::RegistrySequenceExhausted)
        ));
        assert!(matches!(
            registry.admit(&key),
            Err(ServingSlotRegistryError::RegistrySequenceExhausted)
        ));
        assert!(matches!(
            registry.seal_drain_claim_v2(&key, seal_key, None),
            Err(ServingSlotRegistryError::RegistrySequenceExhausted)
        ));
    }

    #[test]
    fn failed_closed_registry_cannot_mint_or_revalidate_an_empty_recovery_cursor() {
        let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1::default());
        let cursor = registry
            .recovery_observation_guard_v2()
            .unwrap()
            .into_empty_cursor()
            .unwrap();
        {
            let mut state = registry.inner.state.lock().unwrap();
            state.observation.failed_closed = true;
        }
        assert_eq!(
            registry.revalidate_empty_recovery_cursor_v2(&cursor),
            Err(ServingSlotRegistryError::RegistryRecoveryNotEmpty)
        );
        let guard = registry.recovery_observation_guard_v2().unwrap();
        assert!(guard.observation().registry_failed_closed());
        assert!(matches!(
            guard.into_empty_cursor(),
            Err(ServingSlotRegistryError::RegistryRecoveryNotEmpty)
        ));
    }
}
