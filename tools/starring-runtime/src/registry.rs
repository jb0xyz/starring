use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};

use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_registry::{
    RegistryEmptyRecoveryCursorV2, RegistryRecoveryObservationGuardV2,
    RegistryRecoveryObservationV2, ServingSlotKeyV1, ServingSlotRegistryConfigV1,
    ServingSlotRegistryError, ServingSlotRegistryV1, SlotAdmissionStateV2, SlotAtomicObservationV2,
    SlotDrainClaimSealV2, SlotSealKeyV2,
};
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryEmptyObservationV2,
    RuntimeRegistryRecoveryObservationErrorV2, RuntimeRegistryRecoveryObservationInputV2,
};

use crate::closed_recovery::RuntimeClosedRecoveryTransitionAuthorityV2;
use crate::gateway::{RuntimeEmergencyGatewaySectionV2, RuntimeRecoveryPendingGatewaySectionV2};
use crate::GatewayResourceConfigV1;

const REGISTRY_MAX_SLOTS: NonZeroU32 = NonZeroU32::new(4_096).unwrap();
const REGISTRY_MAX_RETIRED_ROUTES_PER_SLOT: NonZeroU32 = NonZeroU32::new(8).unwrap();

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRegistryBootstrapErrorV1 {
    #[error("runtime registry active interaction capacity is outside its supported domain")]
    ActiveInteractionCapacity,
}

impl RuntimeRegistryBootstrapErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ActiveInteractionCapacity => "runtime_registry_active_interaction_capacity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRegistryRecoveryObservationErrorV1 {
    #[error("runtime registry is unavailable")]
    RegistryUnavailable,
    #[error("runtime registry recovery observation is invalid")]
    ObservationInvalid,
    #[error("runtime registry recovery observation overflowed")]
    ObservationOverflow,
    #[error("runtime registry recovery observation is failed closed")]
    FailedClosed,
    #[error("runtime registry recovery observation is not empty")]
    NotEmpty,
    #[error("runtime registry recovery retained counts are inconsistent")]
    InconsistentRetainedCounts,
    #[error("runtime registry recovery observation sequence is outside the persistence domain")]
    ObservationSequenceOutOfRange,
    #[error("runtime registry empty recovery binding is stale")]
    StaleEmptyBinding,
    #[error("runtime registry recovery protocol was violated")]
    ProtocolViolation,
}

impl RuntimeRegistryRecoveryObservationErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::RegistryUnavailable => "runtime_registry_unavailable",
            Self::ObservationInvalid => "runtime_registry_observation_invalid",
            Self::ObservationOverflow => "runtime_registry_observation_overflow",
            Self::FailedClosed => "runtime_registry_failed_closed",
            Self::NotEmpty => "runtime_registry_not_empty",
            Self::InconsistentRetainedCounts => "runtime_registry_retained_counts_inconsistent",
            Self::ObservationSequenceOutOfRange => {
                "runtime_registry_observation_sequence_out_of_range"
            }
            Self::StaleEmptyBinding => "runtime_registry_empty_binding_stale",
            Self::ProtocolViolation => "runtime_registry_protocol_violation",
        }
    }
}

pub struct RuntimeRegistryBootstrapV1 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
}

impl RuntimeRegistryBootstrapV1 {
    pub fn observe_recovery_empty_projection_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        self.recovery_observation_guard_unordered_v2()?
            .empty_projection_v2()
    }

    pub(crate) fn recovery_observation_guard_v2(
        &self,
        _authority: &RuntimeClosedRecoveryTransitionAuthorityV2,
        _section: &RuntimeEmergencyGatewaySectionV2<'_>,
    ) -> Result<RuntimeRegistryRecoveryGuardV1<'_>, RuntimeRegistryRecoveryObservationErrorV1> {
        self.recovery_observation_guard_unordered_v2()
    }

    fn recovery_observation_guard_unordered_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryGuardV1<'_>, RuntimeRegistryRecoveryObservationErrorV1> {
        let guard = self
            .registry
            .recovery_observation_guard_v2()
            .map_err(map_registry_observation_error)?;
        Ok(RuntimeRegistryRecoveryGuardV1 {
            bootstrap: self,
            guard,
        })
    }
}

impl Debug for RuntimeRegistryBootstrapV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBootstrapV1(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryRecoveryGuardV1<'a> {
    bootstrap: &'a RuntimeRegistryBootstrapV1,
    guard: RegistryRecoveryObservationGuardV2<'a>,
}

impl<'a> RuntimeRegistryRecoveryGuardV1<'a> {
    fn empty_projection_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        project_empty_observation_v2(
            &self.bootstrap.process_instance_id,
            self.guard.observation(),
        )
    }

    pub(crate) fn locked_empty_evidence_v2<'evidence>(
        &'evidence self,
    ) -> Result<
        RuntimeLockedRegistryEmptyEvidenceV2<'evidence, 'a>,
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        let observation = project_empty_observation_v2(
            &self.bootstrap.process_instance_id,
            self.guard.observation(),
        )?;
        Ok(RuntimeLockedRegistryEmptyEvidenceV2 {
            observation,
            _guard: self,
        })
    }

    pub(crate) fn into_empty_binding_v2(
        self,
    ) -> Result<RuntimeRegistryEmptyRecoveryBindingV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        let cursor = self
            .guard
            .into_empty_cursor()
            .map_err(map_registry_observation_error)?;
        Ok(RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: self.bootstrap.process_instance_id.clone(),
            registry: self.bootstrap.registry.clone(),
            cursor,
        })
    }
}

pub(crate) struct RuntimeLockedRegistryEmptyEvidenceV2<'evidence, 'registry> {
    observation: RuntimeRegistryRecoveryEmptyObservationV2,
    _guard: &'evidence RuntimeRegistryRecoveryGuardV1<'registry>,
}

impl RuntimeLockedRegistryEmptyEvidenceV2<'_, '_> {
    pub(crate) fn into_observation_v2(self) -> RuntimeRegistryRecoveryEmptyObservationV2 {
        self.observation
    }
}

impl Debug for RuntimeLockedRegistryEmptyEvidenceV2<'_, '_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLockedRegistryEmptyEvidenceV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryEmptyRecoveryBindingV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
    cursor: RegistryEmptyRecoveryCursorV2,
}

impl RuntimeRegistryEmptyRecoveryBindingV2 {
    pub(crate) fn revalidate_empty_projection_v2(
        &self,
        _section: &RuntimeRecoveryPendingGatewaySectionV2<'_>,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        self.revalidate_empty_projection_unordered_v2()
    }

    fn revalidate_empty_projection_unordered_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        let observation = self
            .registry
            .revalidate_empty_recovery_cursor_v2(&self.cursor)
            .map_err(map_registry_observation_error)?;
        project_empty_observation_v2(&self.process_instance_id, observation)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn into_pending_drain_seal_binding_v2(
        self,
        key: ServingSlotKeyV1,
        seal_key: SlotSealKeyV2,
    ) -> Result<RuntimeRegistryPendingDrainSealBindingV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        let source_empty_observation = self.revalidate_empty_projection_unordered_v2()?;
        let sealed = self
            .registry
            .seal_empty_recovery_drain_claim_v2(self.cursor, &key, seal_key)
            .map_err(map_registry_observation_error)?;
        let source_slot_observation = sealed.source_slot_observation().cloned();
        let slot_observation = sealed.slot_observation().clone();
        let registry_observation = sealed.registry_observation();
        let seal = sealed.into_seal();
        validate_pending_drain_seal_v2(
            &key,
            seal_key,
            &source_empty_observation,
            source_slot_observation.as_ref(),
            &seal,
            &slot_observation,
            registry_observation,
        )?;
        Ok(RuntimeRegistryPendingDrainSealBindingV2 {
            process_instance_id: self.process_instance_id,
            registry: self.registry,
            seal,
            source_empty_observation,
            source_slot_observation,
            slot_observation,
            registry_observation,
        })
    }
}

impl Debug for RuntimeRegistryEmptyRecoveryBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryEmptyRecoveryBindingV2(<redacted>)")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryPendingDrainSealBindingV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
    seal: SlotDrainClaimSealV2,
    source_empty_observation: RuntimeRegistryRecoveryEmptyObservationV2,
    source_slot_observation: Option<SlotAtomicObservationV2>,
    slot_observation: SlotAtomicObservationV2,
    registry_observation: RegistryRecoveryObservationV2,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryPendingDrainSealBindingV2 {
    pub(crate) fn seal_key_bytes_v2(&self) -> [u8; 16] {
        *self.seal.seal_key().as_bytes()
    }

    pub(crate) fn seal_generation_v2(&self) -> NonZeroU64 {
        self.seal.seal_generation()
    }

    pub(crate) fn source_empty_observation_v2(&self) -> &RuntimeRegistryRecoveryEmptyObservationV2 {
        &self.source_empty_observation
    }

    pub(crate) fn source_slot_is_present_v2(&self) -> bool {
        self.source_slot_observation.is_some()
    }

    pub(crate) fn source_slot_admission_generation_v2(&self) -> Option<NonZeroU64> {
        self.source_slot_observation
            .as_ref()
            .map(|observation| observation.admission_generation)
    }

    pub(crate) fn source_slot_observation_sequence_v2(&self) -> Option<NonZeroU64> {
        self.source_slot_observation
            .as_ref()
            .map(|observation| observation.observation_sequence)
    }

    pub(crate) fn post_seal_admission_generation_v2(&self) -> NonZeroU64 {
        self.slot_observation.admission_generation
    }

    pub(crate) fn post_seal_slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.slot_observation.observation_sequence
    }

    pub(crate) fn post_seal_global_observation_sequence_v2(
        &self,
    ) -> RuntimeRegistryGlobalObservationSequenceV2 {
        RuntimeRegistryGlobalObservationSequenceV2::new(
            self.registry_observation
                .observation_sequence()
                .as_non_zero(),
        )
    }

    pub(crate) fn post_seal_retained_slot_count_v2(&self) -> u64 {
        self.registry_observation.retained_slot_count()
    }

    pub(crate) fn post_seal_retained_empty_tombstone_count_v2(&self) -> u64 {
        self.registry_observation.retained_empty_tombstone_count()
    }

    pub(crate) fn post_seal_staged_route_count_v2(&self) -> u64 {
        self.registry_observation.staged_route_count()
    }

    pub(crate) fn post_seal_serving_route_count_v2(&self) -> u64 {
        self.registry_observation.serving_route_count()
    }

    pub(crate) fn post_seal_draining_route_count_v2(&self) -> u64 {
        self.registry_observation.draining_route_count()
    }

    pub(crate) fn post_seal_sealed_slot_count_v2(&self) -> u64 {
        self.registry_observation.sealed_slot_count()
    }

    pub(crate) fn post_seal_active_interaction_count_v2(&self) -> u64 {
        self.registry_observation.active_interaction_count()
    }

    pub(crate) fn post_seal_failed_closed_slot_count_v2(&self) -> u64 {
        self.registry_observation.failed_closed_slot_count()
    }

    pub(crate) fn post_seal_registry_failed_closed_v2(&self) -> bool {
        self.registry_observation.registry_failed_closed()
    }

    pub(crate) fn revalidate_sealed_v2(
        &self,
    ) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
        let first_registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(map_registry_observation_error)?;
        if first_registry_observation != self.registry_observation {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let slot_observation = self
            .registry
            .atomic_observation_v2(self.seal.key())
            .map_err(map_registry_observation_error)?;
        if slot_observation.as_ref() != Some(&self.slot_observation) {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let second_registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(map_registry_observation_error)?;
        if second_registry_observation != self.registry_observation {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        validate_pending_drain_seal_v2(
            self.seal.key(),
            self.seal.seal_key(),
            &self.source_empty_observation,
            self.source_slot_observation.as_ref(),
            &self.seal,
            &self.slot_observation,
            self.registry_observation,
        )
    }
}

impl Debug for RuntimeRegistryPendingDrainSealBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPendingDrainSealBindingV2(<redacted>)")
    }
}

fn validate_pending_drain_seal_v2(
    key: &ServingSlotKeyV1,
    seal_key: SlotSealKeyV2,
    source_empty_observation: &RuntimeRegistryRecoveryEmptyObservationV2,
    source_slot_observation: Option<&SlotAtomicObservationV2>,
    seal: &SlotDrainClaimSealV2,
    slot_observation: &SlotAtomicObservationV2,
    registry_observation: RegistryRecoveryObservationV2,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    if seal.key() != key
        || seal.seal_key() != seal_key
        || seal.route().is_some()
        || slot_observation.route.is_some()
        || slot_observation.active_interactions != 0
        || slot_observation.admission_state
            != (SlotAdmissionStateV2::DrainClaimSealed {
                seal_key,
                seal_generation: seal.seal_generation(),
            })
        || registry_observation.registry_failed_closed()
        || registry_observation.staged_route_count() != 0
        || registry_observation.serving_route_count() != 0
        || registry_observation.draining_route_count() != 0
        || registry_observation.sealed_slot_count() != 1
        || registry_observation.active_interaction_count() != 0
        || registry_observation.failed_closed_slot_count() != 0
    {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    let expected_registry_sequence = successor_non_zero_u64_v2(
        source_empty_observation
            .observation_sequence()
            .as_non_zero(),
    )?;
    if registry_observation.observation_sequence().as_non_zero() != expected_registry_sequence {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    let source_retained_slot_count = source_empty_observation.retained_slot_count();
    let source_retained_empty_tombstone_count =
        source_empty_observation.retained_empty_tombstone_count();
    if source_retained_slot_count != source_retained_empty_tombstone_count {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    let (expected_retained_slot_count, expected_retained_empty_tombstone_count) =
        if let Some(source_slot_observation) = source_slot_observation {
            if source_slot_observation.route.is_some()
                || source_slot_observation.admission_state != SlotAdmissionStateV2::Empty
                || source_slot_observation.active_interactions != 0
                || slot_observation.admission_generation
                    != successor_non_zero_u64_v2(source_slot_observation.admission_generation)?
                || slot_observation.observation_sequence
                    != successor_non_zero_u64_v2(source_slot_observation.observation_sequence)?
            {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            (
                source_retained_slot_count,
                source_retained_empty_tombstone_count
                    .checked_sub(1)
                    .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?,
            )
        } else {
            if slot_observation.admission_generation != NonZeroU64::MIN
                || slot_observation.observation_sequence != NonZeroU64::MIN
            {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            (
                source_retained_slot_count
                    .checked_add(1)
                    .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?,
                source_retained_empty_tombstone_count,
            )
        };
    if registry_observation.retained_slot_count() != expected_retained_slot_count
        || registry_observation.retained_empty_tombstone_count()
            != expected_retained_empty_tombstone_count
        || registry_observation.retained_slot_count()
            != registry_observation
                .retained_empty_tombstone_count()
                .checked_add(1)
                .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?
    {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    Ok(())
}

fn successor_non_zero_u64_v2(
    value: NonZeroU64,
) -> Result<NonZeroU64, RuntimeRegistryRecoveryObservationErrorV1> {
    value
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)
}

pub fn compose_runtime_registry_bootstrap_v1(
    process_instance_id: ProcessInstanceId,
    gateway: GatewayResourceConfigV1,
) -> Result<RuntimeRegistryBootstrapV1, RuntimeRegistryBootstrapErrorV1> {
    let max_active_interactions_per_slot =
        registry_active_interaction_capacity(gateway.global_admission_capacity())?;
    let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: REGISTRY_MAX_SLOTS,
        max_active_interactions_per_slot,
        max_retired_routes_per_slot: REGISTRY_MAX_RETIRED_ROUTES_PER_SLOT,
    });
    Ok(RuntimeRegistryBootstrapV1 {
        process_instance_id,
        registry,
    })
}

fn registry_active_interaction_capacity(
    capacity: NonZeroUsize,
) -> Result<NonZeroU32, RuntimeRegistryBootstrapErrorV1> {
    u32::try_from(capacity.get())
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or(RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity)
}

fn project_empty_observation_v2(
    process_instance_id: &ProcessInstanceId,
    observation: RegistryRecoveryObservationV2,
) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
    let observation_sequence = NonZeroU64::new(observation.observation_sequence().get())
        .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?;
    accept_runtime_registry_recovery_empty_observation_v2(
        process_instance_id.clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                observation_sequence,
            ),
            retained_slot_count: observation.retained_slot_count(),
            retained_empty_tombstone_count: observation.retained_empty_tombstone_count(),
            staged_route_count: observation.staged_route_count(),
            serving_route_count: observation.serving_route_count(),
            draining_route_count: observation.draining_route_count(),
            sealed_slot_count: observation.sealed_slot_count(),
            active_interaction_count: observation.active_interaction_count(),
            failed_closed_slot_count: observation.failed_closed_slot_count(),
            registry_failed_closed: observation.registry_failed_closed(),
        },
    )
    .map_err(map_worker_observation_error)
}

fn map_registry_observation_error(
    error: ServingSlotRegistryError,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    match error {
        ServingSlotRegistryError::RegistryPoisoned => {
            RuntimeRegistryRecoveryObservationErrorV1::RegistryUnavailable
        }
        ServingSlotRegistryError::RegistryObservationInvalid => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationInvalid
        }
        ServingSlotRegistryError::RegistryObservationOverflow => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow
        }
        ServingSlotRegistryError::RegistryRecoveryNotEmpty => {
            RuntimeRegistryRecoveryObservationErrorV1::NotEmpty
        }
        ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor => {
            RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding
        }
        ServingSlotRegistryError::TargetSlotMismatch
        | ServingSlotRegistryError::StaleFencingToken { .. }
        | ServingSlotRegistryError::StaleRuntimeGeneration { .. }
        | ServingSlotRegistryError::RuntimeGenerationIdentityConflict
        | ServingSlotRegistryError::AuthorityTargetMismatch
        | ServingSlotRegistryError::NonSuccessorFencingToken { .. }
        | ServingSlotRegistryError::FencingTokenExhausted
        | ServingSlotRegistryError::StaleMutationToken
        | ServingSlotRegistryError::ActivationTargetMismatch
        | ServingSlotRegistryError::NotServing
        | ServingSlotRegistryError::ActiveInteractionCapacityExceeded
        | ServingSlotRegistryError::NotDraining
        | ServingSlotRegistryError::ActiveInteractionsRemain { .. }
        | ServingSlotRegistryError::RetiredRouteCapacityExceeded
        | ServingSlotRegistryError::SlotCapacityExceeded
        | ServingSlotRegistryError::IncarnationExhausted
        | ServingSlotRegistryError::SlotSequenceExhausted
        | ServingSlotRegistryError::RegistrySequenceExhausted
        | ServingSlotRegistryError::AdmissionGenerationMismatch { .. }
        | ServingSlotRegistryError::StaleSlotObservation
        | ServingSlotRegistryError::SlotSealed
        | ServingSlotRegistryError::StaleSlotSeal => {
            RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation
        }
    }
}

fn map_worker_observation_error(
    error: RuntimeRegistryRecoveryObservationErrorV2,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    match error {
        RuntimeRegistryRecoveryObservationErrorV2::FailedClosed => {
            RuntimeRegistryRecoveryObservationErrorV1::FailedClosed
        }
        RuntimeRegistryRecoveryObservationErrorV2::ObservationSequenceOutOfRange => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange
        }
        RuntimeRegistryRecoveryObservationErrorV2::NotEmpty => {
            RuntimeRegistryRecoveryObservationErrorV1::NotEmpty
        }
        RuntimeRegistryRecoveryObservationErrorV2::InconsistentRetainedCounts => {
            RuntimeRegistryRecoveryObservationErrorV1::InconsistentRetainedCounts
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use automation_runtime_convergence::{ProcessInstanceId, RuntimeDeploymentTargetV1};
    use automation_runtime_registry::{ServingSlotKeyV1, ServingSlotRegistryError, SlotSealKeyV2};
    use automation_runtime_worker::RuntimeRegistryRecoveryObservationErrorV2;
    use serde_json::json;

    use super::{
        compose_runtime_registry_bootstrap_v1, map_registry_observation_error,
        map_worker_observation_error, registry_active_interaction_capacity,
        RuntimeRegistryBootstrapErrorV1, RuntimeRegistryEmptyRecoveryBindingV2,
        RuntimeRegistryRecoveryObservationErrorV1,
    };
    use crate::GatewayResourceConfigV1;

    fn slot_key() -> ServingSlotKeyV1 {
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap();
        ServingSlotKeyV1::from_target(&target)
    }

    fn seal_key() -> SlotSealKeyV2 {
        SlotSealKeyV2::try_from([7_u8; 16].as_slice()).unwrap()
    }

    impl super::RuntimeRegistryBootstrapV1 {
        pub(crate) fn advance_empty_sequence_for_test_v2(&self) {
            let key = slot_key();
            let expected = self.registry.atomic_observation_v2(&key).unwrap();
            let (seal, _) = self
                .registry
                .seal_drain_claim_v2(&key, seal_key(), expected.as_ref())
                .unwrap();
            self.registry.unseal_drain_claim_v2(seal).unwrap();
        }
    }

    impl super::RuntimeRegistryPendingDrainSealBindingV2 {
        fn into_empty_binding_after_durable_ack_v2(
            self,
        ) -> Result<
            (
                RuntimeRegistryEmptyRecoveryBindingV2,
                automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2,
            ),
            RuntimeRegistryRecoveryObservationErrorV1,
        > {
            self.revalidate_sealed_v2()?;
            let expected_slot_observation_sequence =
                super::successor_non_zero_u64_v2(self.slot_observation.observation_sequence)?;
            let expected_admission_generation =
                super::successor_non_zero_u64_v2(self.slot_observation.admission_generation)?;
            let expected_registry_observation_sequence = super::successor_non_zero_u64_v2(
                self.registry_observation
                    .observation_sequence()
                    .as_non_zero(),
            )?;
            let expected_retained_slot_count = self.registry_observation.retained_slot_count();
            let expected_retained_empty_tombstone_count = self
                .registry_observation
                .retained_empty_tombstone_count()
                .checked_add(1)
                .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?;
            let super::RuntimeRegistryPendingDrainSealBindingV2 {
                process_instance_id,
                registry,
                seal,
                ..
            } = self;
            let unsealed = registry
                .unseal_empty_recovery_drain_claim_v2(seal)
                .map_err(super::map_registry_observation_error)?;
            let slot_observation = unsealed.slot_observation();
            if slot_observation.route.is_some()
                || slot_observation.admission_state
                    != automation_runtime_registry::SlotAdmissionStateV2::Empty
                || slot_observation.active_interactions != 0
                || slot_observation.admission_generation != expected_admission_generation
                || slot_observation.observation_sequence != expected_slot_observation_sequence
            {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            let registry_observation = unsealed.registry_observation();
            if registry_observation.observation_sequence().as_non_zero()
                != expected_registry_observation_sequence
                || registry_observation.retained_slot_count() != expected_retained_slot_count
                || registry_observation.retained_empty_tombstone_count()
                    != expected_retained_empty_tombstone_count
            {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            let empty_observation =
                super::project_empty_observation_v2(&process_instance_id, registry_observation)?;
            let binding = RuntimeRegistryEmptyRecoveryBindingV2 {
                process_instance_id,
                registry,
                cursor: unsealed.into_cursor(),
            };
            if binding.revalidate_empty_projection_unordered_v2()? != empty_observation {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            Ok((binding, empty_observation))
        }
    }

    #[test]
    fn composes_exact_empty_projection_without_exposing_registry_authority() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();

        let projection = bootstrap.observe_recovery_empty_projection_v2().unwrap();

        assert_eq!(
            projection.process_instance_id().as_str(),
            "runtime-process:1"
        );
        assert_eq!(projection.observation_sequence().get(), 1);
        assert_eq!(projection.retained_slot_count(), 0);
        assert_eq!(projection.retained_empty_tombstone_count(), 0);
        assert_eq!(
            format!("{bootstrap:?}"),
            "RuntimeRegistryBootstrapV1(<redacted>)"
        );
    }

    #[test]
    fn private_empty_binding_revalidates_the_exact_bootstrap_and_sequence() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let projected = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        assert_eq!(
            binding.revalidate_empty_projection_unordered_v2().unwrap(),
            projected
        );
        assert_eq!(
            format!("{binding:?}"),
            "RuntimeRegistryEmptyRecoveryBindingV2(<redacted>)"
        );
    }

    #[test]
    fn private_empty_binding_rejects_sequence_aba_and_foreign_registry() {
        let process_instance_id = ProcessInstanceId::parse("runtime-process:1").unwrap();
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let key = slot_key();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(&key, seal_key(), None)
            .unwrap();
        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
        let stale = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        let before = bootstrap.observe_recovery_empty_projection_v2().unwrap();
        let expected = bootstrap.registry.atomic_observation_v2(&key).unwrap();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(&key, seal_key(), expected.as_ref())
            .unwrap();
        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
        let after = bootstrap.observe_recovery_empty_projection_v2().unwrap();

        assert_eq!(before.retained_slot_count(), after.retained_slot_count());
        assert_eq!(
            before.retained_empty_tombstone_count(),
            after.retained_empty_tombstone_count()
        );
        assert_ne!(before.observation_sequence(), after.observation_sequence());

        assert_eq!(
            stale.revalidate_empty_projection_unordered_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding)
        );

        let source = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let foreign = compose_runtime_registry_bootstrap_v1(
            process_instance_id,
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let cursor = source
            .registry
            .recovery_observation_guard_v2()
            .unwrap()
            .into_empty_cursor()
            .unwrap();
        let foreign_binding = RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: foreign.process_instance_id.clone(),
            registry: foreign.registry.clone(),
            cursor,
        };

        assert_eq!(
            foreign_binding.revalidate_empty_projection_unordered_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding)
        );
    }

    #[test]
    fn pending_drain_seal_binding_tracks_absent_slot_s0_s1_s2_exactly() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let source = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        let sealed = binding
            .into_pending_drain_seal_binding_v2(slot_key(), seal_key())
            .unwrap();

        assert_eq!(sealed.source_empty_observation_v2(), &source);
        assert_eq!(sealed.seal_key_bytes_v2(), [7_u8; 16]);
        assert_eq!(sealed.seal_generation_v2().get(), 1);
        assert!(!sealed.source_slot_is_present_v2());
        assert_eq!(sealed.source_slot_admission_generation_v2(), None);
        assert_eq!(sealed.source_slot_observation_sequence_v2(), None);
        assert_eq!(sealed.post_seal_admission_generation_v2().get(), 1);
        assert_eq!(sealed.post_seal_slot_observation_sequence_v2().get(), 1);
        assert_eq!(
            sealed.post_seal_global_observation_sequence_v2().get(),
            source.observation_sequence().get() + 1
        );
        assert_eq!(sealed.post_seal_retained_slot_count_v2(), 1);
        assert_eq!(sealed.post_seal_retained_empty_tombstone_count_v2(), 0);
        assert_eq!(sealed.post_seal_staged_route_count_v2(), 0);
        assert_eq!(sealed.post_seal_serving_route_count_v2(), 0);
        assert_eq!(sealed.post_seal_draining_route_count_v2(), 0);
        assert_eq!(sealed.post_seal_sealed_slot_count_v2(), 1);
        assert_eq!(sealed.post_seal_active_interaction_count_v2(), 0);
        assert_eq!(sealed.post_seal_failed_closed_slot_count_v2(), 0);
        assert!(!sealed.post_seal_registry_failed_closed_v2());
        assert_eq!(
            format!("{sealed:?}"),
            "RuntimeRegistryPendingDrainSealBindingV2(<redacted>)"
        );
        sealed.revalidate_sealed_v2().unwrap();

        let (restored, successor) = sealed.into_empty_binding_after_durable_ack_v2().unwrap();

        assert_eq!(
            successor.observation_sequence().get(),
            source.observation_sequence().get() + 2
        );
        assert_eq!(successor.retained_slot_count(), 1);
        assert_eq!(successor.retained_empty_tombstone_count(), 1);
        assert_eq!(
            restored.revalidate_empty_projection_unordered_v2().unwrap(),
            successor
        );
    }

    #[test]
    fn pending_drain_seal_binding_preserves_tombstone_local_generation() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        bootstrap.advance_empty_sequence_for_test_v2();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let source = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        let sealed = binding
            .into_pending_drain_seal_binding_v2(slot_key(), seal_key())
            .unwrap();

        assert!(sealed.source_slot_is_present_v2());
        assert_eq!(
            sealed.source_slot_admission_generation_v2().unwrap().get(),
            2
        );
        assert_eq!(
            sealed.source_slot_observation_sequence_v2().unwrap().get(),
            2
        );
        assert_eq!(sealed.seal_generation_v2().get(), 2);
        assert_eq!(sealed.post_seal_admission_generation_v2().get(), 3);
        assert_eq!(sealed.post_seal_slot_observation_sequence_v2().get(), 3);
        assert_eq!(sealed.post_seal_retained_slot_count_v2(), 1);
        assert_eq!(sealed.post_seal_retained_empty_tombstone_count_v2(), 0);
        assert_eq!(
            sealed.post_seal_global_observation_sequence_v2().get(),
            source.observation_sequence().get() + 1
        );

        let (_, successor) = sealed.into_empty_binding_after_durable_ack_v2().unwrap();
        assert_eq!(successor.retained_slot_count(), 1);
        assert_eq!(successor.retained_empty_tombstone_count(), 1);
    }

    #[test]
    fn pending_drain_seal_rejects_stale_and_foreign_s0_bindings() {
        let process_instance_id = ProcessInstanceId::parse("runtime-process:1").unwrap();
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let stale = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        bootstrap.advance_empty_sequence_for_test_v2();

        assert_eq!(
            stale
                .into_pending_drain_seal_binding_v2(slot_key(), seal_key())
                .err()
                .unwrap(),
            RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding
        );

        let source = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let foreign = compose_runtime_registry_bootstrap_v1(
            process_instance_id,
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let cursor = source
            .registry
            .recovery_observation_guard_v2()
            .unwrap()
            .into_empty_cursor()
            .unwrap();
        let foreign_binding = RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: foreign.process_instance_id.clone(),
            registry: foreign.registry.clone(),
            cursor,
        };

        assert_eq!(
            foreign_binding
                .into_pending_drain_seal_binding_v2(slot_key(), seal_key())
                .err()
                .unwrap(),
            RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding
        );
    }

    #[test]
    fn pending_drain_seal_exposes_no_mutable_capability_and_drop_stays_sealed() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let binding = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        let sealed = binding
            .into_pending_drain_seal_binding_v2(slot_key(), seal_key())
            .unwrap();
        let mut exposed_key = sealed.seal_key_bytes_v2();
        exposed_key[0] = 99;

        assert_eq!(exposed_key[0], 99);
        assert_eq!(sealed.seal_key_bytes_v2(), [7_u8; 16]);
        sealed.revalidate_sealed_v2().unwrap();
        drop(sealed);
        assert_eq!(
            bootstrap.observe_recovery_empty_projection_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
        );
        let observation = bootstrap
            .registry
            .atomic_observation_v2(&slot_key())
            .unwrap()
            .unwrap();
        assert!(matches!(
            observation.admission_state,
            automation_runtime_registry::SlotAdmissionStateV2::DrainClaimSealed { .. }
        ));
    }

    #[test]
    fn registry_observation_errors_are_closed_and_stable() {
        for (source, expected) in [
            (
                ServingSlotRegistryError::RegistryPoisoned,
                RuntimeRegistryRecoveryObservationErrorV1::RegistryUnavailable,
            ),
            (
                ServingSlotRegistryError::RegistryObservationInvalid,
                RuntimeRegistryRecoveryObservationErrorV1::ObservationInvalid,
            ),
            (
                ServingSlotRegistryError::RegistryObservationOverflow,
                RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow,
            ),
            (
                ServingSlotRegistryError::RegistryRecoveryNotEmpty,
                RuntimeRegistryRecoveryObservationErrorV1::NotEmpty,
            ),
            (
                ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor,
                RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
            ),
            (
                ServingSlotRegistryError::RegistrySequenceExhausted,
                RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation,
            ),
        ] {
            assert_eq!(map_registry_observation_error(source), expected);
        }
    }

    #[test]
    fn sealed_empty_slot_blocks_projection_and_unseal_preserves_tombstone_evidence() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(&slot_key(), seal_key(), None)
            .unwrap();

        assert_eq!(
            bootstrap.observe_recovery_empty_projection_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
        );

        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
        let projection = bootstrap.observe_recovery_empty_projection_v2().unwrap();
        assert_eq!(projection.retained_slot_count(), 1);
        assert_eq!(projection.retained_empty_tombstone_count(), 1);
        assert!(projection.observation_sequence().get() > 1);
    }

    #[test]
    fn worker_observation_errors_preserve_their_failure_class() {
        for (source, expected) in [
            (
                RuntimeRegistryRecoveryObservationErrorV2::FailedClosed,
                RuntimeRegistryRecoveryObservationErrorV1::FailedClosed,
            ),
            (
                RuntimeRegistryRecoveryObservationErrorV2::ObservationSequenceOutOfRange,
                RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange,
            ),
            (
                RuntimeRegistryRecoveryObservationErrorV2::NotEmpty,
                RuntimeRegistryRecoveryObservationErrorV1::NotEmpty,
            ),
            (
                RuntimeRegistryRecoveryObservationErrorV2::InconsistentRetainedCounts,
                RuntimeRegistryRecoveryObservationErrorV1::InconsistentRetainedCounts,
            ),
        ] {
            assert_eq!(map_worker_observation_error(source), expected);
        }
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(
            RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity.code(),
            "runtime_registry_active_interaction_capacity"
        );
        assert_eq!(
            RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation.code(),
            "runtime_registry_protocol_violation"
        );
    }

    #[test]
    fn active_interaction_capacity_conversion_is_checked_at_both_boundaries() {
        assert_eq!(
            registry_active_interaction_capacity(NonZeroUsize::new(u32::MAX as usize).unwrap())
                .unwrap()
                .get(),
            u32::MAX
        );
        if usize::BITS > u32::BITS {
            assert_eq!(
                registry_active_interaction_capacity(
                    NonZeroUsize::new(u32::MAX as usize + 1).unwrap()
                ),
                Err(RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity)
            );
        }
    }
}
