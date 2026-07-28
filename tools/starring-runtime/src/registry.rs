use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};

use automation_runtime_controller::RuntimeServingSlotV2;
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_registry::{
    RegistryEmptyRecoveryCursorV2, RegistryRecoveryObservationGuardV2,
    RegistryRecoveryObservationV2, SealedEmptyRecoveryDrainClaimV2, ServingSlotKeyV1,
    ServingSlotRegistryConfigV1, ServingSlotRegistryError, ServingSlotRegistryV1,
    SlotAdmissionStateV2, SlotSealKeyV2,
};
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2,
    RuntimeDurablyAcknowledgedPendingDrainSuccessionV3, RuntimeDurablyAcknowledgedPendingDrainV2,
    RuntimePendingDrainCandidateV2, RuntimePendingDrainCompoundErrorV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    RuntimePendingDrainRegistrySealWitnessInputV2, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainRegistryUnsealWitnessV2, RuntimePendingDrainSlotObservationV2,
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

    pub(crate) fn into_pending_drain_seal_binding_v2(
        self,
        candidate: &RuntimePendingDrainCandidateV2,
    ) -> Result<
        (
            RuntimeRegistryPendingDrainSealBindingV2,
            RuntimePendingDrainRegistrySealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.into_pending_drain_seal_binding_common_v2(
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            SlotSealKeyV2::try_from(candidate.intent_id().canonical_bytes().as_slice())
                .map_err(|_| RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?,
            candidate.slot(),
        )
    }

    pub(crate) fn into_pending_drain_succession_seal_binding_v3(
        self,
        candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    ) -> Result<
        (
            RuntimeRegistryPendingDrainSuccessionSealBindingV3,
            RuntimePendingDrainRegistrySealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        let (binding, witness) = self.into_pending_drain_seal_binding_common_v2(
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            SlotSealKeyV2::try_from(candidate.intent_id().canonical_bytes().as_slice())
                .map_err(|_| RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?,
            candidate.slot(),
        )?;
        Ok((
            RuntimeRegistryPendingDrainSuccessionSealBindingV3 { binding },
            witness,
        ))
    }

    fn into_pending_drain_seal_binding_common_v2(
        self,
        key: ServingSlotKeyV1,
        seal_key: SlotSealKeyV2,
        slot: &RuntimeServingSlotV2,
    ) -> Result<
        (
            RuntimeRegistryPendingDrainSealBindingV2,
            RuntimePendingDrainRegistrySealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        let source_empty_observation = self.revalidate_empty_projection_unordered_v2()?;
        let sealed = self
            .registry
            .seal_empty_recovery_drain_claim_v2(self.cursor, &key, seal_key)
            .map_err(map_registry_observation_error)?;
        validate_pending_drain_seal_v2(&key, seal_key, &source_empty_observation, &sealed)?;
        let witness = pending_drain_seal_witness_v2(
            &self.process_instance_id,
            slot,
            &source_empty_observation,
            &sealed,
        )?;
        let binding = RuntimeRegistryPendingDrainSealBindingV2 {
            process_instance_id: self.process_instance_id,
            registry: self.registry,
            sealed,
            source_empty_observation,
            witness: witness.clone(),
        };
        Ok((binding, witness))
    }
}

impl Debug for RuntimeRegistryEmptyRecoveryBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryEmptyRecoveryBindingV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryPendingDrainSealBindingV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
    sealed: SealedEmptyRecoveryDrainClaimV2,
    source_empty_observation: RuntimeRegistryRecoveryEmptyObservationV2,
    witness: RuntimePendingDrainRegistrySealWitnessV2,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryPendingDrainSealBindingV2 {
    pub(crate) fn seal_key_bytes_v2(&self) -> [u8; 16] {
        *self.sealed.seal().seal_key().as_bytes()
    }

    pub(crate) fn seal_generation_v2(&self) -> NonZeroU64 {
        self.sealed.seal().seal_generation()
    }

    pub(crate) fn source_empty_observation_v2(&self) -> &RuntimeRegistryRecoveryEmptyObservationV2 {
        &self.source_empty_observation
    }

    pub(crate) fn source_slot_is_present_v2(&self) -> bool {
        self.sealed.source_slot_observation().is_some()
    }

    pub(crate) fn source_slot_admission_generation_v2(&self) -> Option<NonZeroU64> {
        self.sealed
            .source_slot_observation()
            .map(|observation| observation.admission_generation)
    }

    pub(crate) fn source_slot_observation_sequence_v2(&self) -> Option<NonZeroU64> {
        self.sealed
            .source_slot_observation()
            .map(|observation| observation.observation_sequence)
    }

    pub(crate) fn post_seal_admission_generation_v2(&self) -> NonZeroU64 {
        self.sealed.slot_observation().admission_generation
    }

    pub(crate) fn post_seal_slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.sealed.slot_observation().observation_sequence
    }

    pub(crate) fn post_seal_global_observation_sequence_v2(
        &self,
    ) -> RuntimeRegistryGlobalObservationSequenceV2 {
        RuntimeRegistryGlobalObservationSequenceV2::new(
            self.sealed
                .registry_observation()
                .observation_sequence()
                .as_non_zero(),
        )
    }

    pub(crate) fn post_seal_retained_slot_count_v2(&self) -> u64 {
        self.sealed.registry_observation().retained_slot_count()
    }

    pub(crate) fn post_seal_retained_empty_tombstone_count_v2(&self) -> u64 {
        self.sealed
            .registry_observation()
            .retained_empty_tombstone_count()
    }

    pub(crate) fn post_seal_staged_route_count_v2(&self) -> u64 {
        self.sealed.registry_observation().staged_route_count()
    }

    pub(crate) fn post_seal_serving_route_count_v2(&self) -> u64 {
        self.sealed.registry_observation().serving_route_count()
    }

    pub(crate) fn post_seal_draining_route_count_v2(&self) -> u64 {
        self.sealed.registry_observation().draining_route_count()
    }

    pub(crate) fn post_seal_sealed_slot_count_v2(&self) -> u64 {
        self.sealed.registry_observation().sealed_slot_count()
    }

    pub(crate) fn post_seal_active_interaction_count_v2(&self) -> u64 {
        self.sealed
            .registry_observation()
            .active_interaction_count()
    }

    pub(crate) fn post_seal_failed_closed_slot_count_v2(&self) -> u64 {
        self.sealed
            .registry_observation()
            .failed_closed_slot_count()
    }

    pub(crate) fn post_seal_registry_failed_closed_v2(&self) -> bool {
        self.sealed.registry_observation().registry_failed_closed()
    }

    pub(crate) fn revalidate_sealed_v2(
        &self,
    ) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
        let first_registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(map_registry_observation_error)?;
        if first_registry_observation != self.sealed.registry_observation() {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let slot_observation = self
            .registry
            .atomic_observation_v2(self.sealed.seal().key())
            .map_err(map_registry_observation_error)?;
        if slot_observation.as_ref() != Some(self.sealed.slot_observation()) {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let second_registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(map_registry_observation_error)?;
        if second_registry_observation != self.sealed.registry_observation() {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        validate_pending_drain_seal_v2(
            self.sealed.seal().key(),
            self.sealed.seal().seal_key(),
            &self.source_empty_observation,
            &self.sealed,
        )
    }

    pub(crate) fn into_empty_binding_after_durable_ack_v2(
        self,
        durable: &RuntimeDurablyAcknowledgedPendingDrainV2,
    ) -> Result<
        (
            RuntimeRegistryEmptyRecoveryBindingV2,
            RuntimePendingDrainRegistryUnsealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.into_empty_binding_after_durable_seal_common_v2(durable.seal_witness())
    }

    fn into_empty_binding_after_durable_seal_common_v2(
        self,
        durable_seal: &RuntimePendingDrainRegistrySealWitnessV2,
    ) -> Result<
        (
            RuntimeRegistryEmptyRecoveryBindingV2,
            RuntimePendingDrainRegistryUnsealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        require_pending_drain_durable_seal_match_v2(&self.witness, durable_seal)?;
        self.revalidate_sealed_v2()?;
        let expected_slot_observation_sequence = successor_persistence_non_zero_u64_v2(
            self.sealed.slot_observation().observation_sequence,
        )?;
        let expected_admission_generation = successor_persistence_non_zero_u64_v2(
            self.sealed.slot_observation().admission_generation,
        )?;
        let expected_registry_observation_sequence = successor_persistence_non_zero_u64_v2(
            self.sealed
                .registry_observation()
                .observation_sequence()
                .as_non_zero(),
        )?;
        let expected_retained_slot_count = self.sealed.registry_observation().retained_slot_count();
        let expected_retained_empty_tombstone_count = self
            .sealed
            .registry_observation()
            .retained_empty_tombstone_count()
            .checked_add(1)
            .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?;
        let expected_empty_observation = accept_runtime_registry_recovery_empty_observation_v2(
            self.process_instance_id.clone(),
            RuntimeRegistryRecoveryObservationInputV2 {
                observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                    expected_registry_observation_sequence,
                ),
                retained_slot_count: expected_retained_slot_count,
                retained_empty_tombstone_count: expected_retained_empty_tombstone_count,
                staged_route_count: 0,
                serving_route_count: 0,
                draining_route_count: 0,
                sealed_slot_count: 0,
                active_interaction_count: 0,
                failed_closed_slot_count: 0,
                registry_failed_closed: false,
            },
        )
        .map_err(map_worker_observation_error)?;
        let preflight_witness = RuntimePendingDrainRegistryUnsealWitnessV2::new(
            self.process_instance_id.clone(),
            durable_seal.slot().clone(),
            expected_admission_generation,
            expected_slot_observation_sequence,
            expected_empty_observation,
        )
        .map_err(map_pending_drain_compound_error)?;
        drop(preflight_witness);
        let Self {
            process_instance_id,
            registry,
            sealed,
            ..
        } = self;
        let unsealed = registry
            .unseal_empty_recovery_drain_claim_v2(sealed)
            .map_err(map_registry_observation_error)?;
        let slot_observation = unsealed.slot_observation();
        if slot_observation.route.is_some()
            || slot_observation.admission_state != SlotAdmissionStateV2::Empty
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
        let projected_empty_observation =
            project_empty_observation_v2(&process_instance_id, registry_observation)?;
        let binding = RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: process_instance_id.clone(),
            registry,
            cursor: unsealed.into_cursor(),
        };
        if binding.revalidate_empty_projection_unordered_v2()? != projected_empty_observation {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let witness = RuntimePendingDrainRegistryUnsealWitnessV2::new(
            process_instance_id,
            durable_seal.slot().clone(),
            expected_admission_generation,
            expected_slot_observation_sequence,
            projected_empty_observation,
        )
        .map_err(map_pending_drain_compound_error)?;
        Ok((binding, witness))
    }
}

fn require_pending_drain_durable_seal_match_v2(
    actual: &RuntimePendingDrainRegistrySealWitnessV2,
    authorized: &RuntimePendingDrainRegistrySealWitnessV2,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    if actual != authorized {
        Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)
    } else {
        Ok(())
    }
}

impl Debug for RuntimeRegistryPendingDrainSealBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPendingDrainSealBindingV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryPendingDrainSuccessionSealBindingV3 {
    binding: RuntimeRegistryPendingDrainSealBindingV2,
}

impl RuntimeRegistryPendingDrainSuccessionSealBindingV3 {
    pub(crate) fn revalidate_sealed_v3(
        &self,
    ) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
        self.binding.revalidate_sealed_v2()
    }

    pub(crate) fn into_empty_binding_after_durable_succession_v3(
        self,
        durable: &RuntimeDurablyAcknowledgedPendingDrainSuccessionV3,
    ) -> Result<
        (
            RuntimeRegistryEmptyRecoveryBindingV2,
            RuntimePendingDrainRegistryUnsealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.binding
            .into_empty_binding_after_durable_seal_common_v2(durable.seal_witness())
    }
}

impl Debug for RuntimeRegistryPendingDrainSuccessionSealBindingV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPendingDrainSuccessionSealBindingV3(<redacted>)")
    }
}

fn validate_pending_drain_seal_v2(
    key: &ServingSlotKeyV1,
    seal_key: SlotSealKeyV2,
    source_empty_observation: &RuntimeRegistryRecoveryEmptyObservationV2,
    sealed: &SealedEmptyRecoveryDrainClaimV2,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    let seal = sealed.seal();
    let source_slot_observation = sealed.source_slot_observation();
    let slot_observation = sealed.slot_observation();
    let registry_observation = sealed.registry_observation();
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

fn pending_drain_seal_witness_v2(
    process_instance_id: &ProcessInstanceId,
    slot: &RuntimeServingSlotV2,
    source_empty_observation: &RuntimeRegistryRecoveryEmptyObservationV2,
    sealed: &SealedEmptyRecoveryDrainClaimV2,
) -> Result<RuntimePendingDrainRegistrySealWitnessV2, RuntimeRegistryRecoveryObservationErrorV1> {
    let source_slot_observation =
        sealed
            .source_slot_observation()
            .map(|observation| RuntimePendingDrainSlotObservationV2 {
                admission_generation: observation.admission_generation,
                observation_sequence: observation.observation_sequence,
            });
    let registry_observation = sealed.registry_observation();
    RuntimePendingDrainRegistrySealWitnessV2::new(RuntimePendingDrainRegistrySealWitnessInputV2 {
        process_instance_id: process_instance_id.clone(),
        slot: slot.clone(),
        pre_slot_observation: source_slot_observation,
        seal_key: *sealed.seal().seal_key().as_bytes(),
        seal_generation: sealed.seal().seal_generation(),
        post_slot_admission_generation: sealed.slot_observation().admission_generation,
        post_slot_observation_sequence: sealed.slot_observation().observation_sequence,
        pre_registry_observation_sequence: source_empty_observation.observation_sequence(),
        pre_registry_retained_slot_count: source_empty_observation.retained_slot_count(),
        pre_registry_retained_empty_tombstone_count: source_empty_observation
            .retained_empty_tombstone_count(),
        post_registry_observation: RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                registry_observation.observation_sequence().as_non_zero(),
            ),
            retained_slot_count: registry_observation.retained_slot_count(),
            retained_empty_tombstone_count: registry_observation.retained_empty_tombstone_count(),
            staged_route_count: registry_observation.staged_route_count(),
            serving_route_count: registry_observation.serving_route_count(),
            draining_route_count: registry_observation.draining_route_count(),
            sealed_slot_count: registry_observation.sealed_slot_count(),
            active_interaction_count: registry_observation.active_interaction_count(),
            failed_closed_slot_count: registry_observation.failed_closed_slot_count(),
            registry_failed_closed: registry_observation.registry_failed_closed(),
        },
    })
    .map_err(map_pending_drain_compound_error)
}

fn map_pending_drain_compound_error(
    _error: RuntimePendingDrainCompoundErrorV2,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation
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

fn successor_persistence_non_zero_u64_v2(
    value: NonZeroU64,
) -> Result<NonZeroU64, RuntimeRegistryRecoveryObservationErrorV1> {
    let successor = successor_non_zero_u64_v2(value)?;
    if successor.get() > i64::MAX as u64 {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange);
    }
    Ok(successor)
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
#[path = "registry_succession_tests.rs"]
mod succession_tests;

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU64, NonZeroUsize};

    use automation_runtime_controller::{RuntimeDrainIntentIdV2, RuntimeServingSlotV2};
    use automation_runtime_convergence::{ProcessInstanceId, RuntimeDeploymentTargetV1};
    use automation_runtime_registry::{ServingSlotKeyV1, ServingSlotRegistryError, SlotSealKeyV2};
    use automation_runtime_worker::{
        RuntimePendingDrainCandidateV2, RuntimePendingDrainStateDigestV2,
        RuntimeRegistryRecoveryObservationErrorV2,
    };
    use serde_json::json;

    use super::{
        compose_runtime_registry_bootstrap_v1, map_registry_observation_error,
        map_worker_observation_error, registry_active_interaction_capacity,
        RuntimeRegistryBootstrapErrorV1, RuntimeRegistryEmptyRecoveryBindingV2,
        RuntimeRegistryRecoveryObservationErrorV1,
    };
    use crate::GatewayResourceConfigV1;

    fn target() -> RuntimeDeploymentTargetV1 {
        serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap()
    }

    fn slot_key() -> ServingSlotKeyV1 {
        ServingSlotKeyV1::from_target(&target())
    }

    fn candidate() -> RuntimePendingDrainCandidateV2 {
        let target = target();
        RuntimePendingDrainCandidateV2::new(
            RuntimeDrainIntentIdV2::parse("07".repeat(16)).unwrap(),
            RuntimeServingSlotV2::from_target(&target),
            target,
            NonZeroU64::new(1).unwrap(),
            RuntimePendingDrainStateDigestV2::new([8; 32]).unwrap(),
        )
        .unwrap()
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
    fn pending_drain_seal_binding_tracks_absent_slot_s0_s1_exactly() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let source = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        let (sealed, witness) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
            .unwrap();

        assert_eq!(sealed.source_empty_observation_v2(), &source);
        assert_eq!(&witness, &sealed.witness);
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

        let (sealed, _) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
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
                .into_pending_drain_seal_binding_v2(&candidate())
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
                .into_pending_drain_seal_binding_v2(&candidate())
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
        let (sealed, _) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
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
    fn pending_drain_durable_token_seal_mismatch_precedes_registry_mutation() {
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
        let (sealed, witness) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
            .unwrap();
        let mismatched = automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2::new(
            automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessInputV2 {
                process_instance_id: witness.process_instance_id().clone(),
                slot: witness.slot().clone(),
                pre_slot_observation: witness.pre_slot_observation(),
                seal_key: [9; 16],
                seal_generation: witness.seal_generation(),
                post_slot_admission_generation: witness.post_slot_admission_generation(),
                post_slot_observation_sequence: witness.post_slot_observation_sequence(),
                pre_registry_observation_sequence: witness.pre_registry_observation_sequence(),
                pre_registry_retained_slot_count: witness.pre_registry_retained_slot_count(),
                pre_registry_retained_empty_tombstone_count: witness
                    .pre_registry_retained_empty_tombstone_count(),
                post_registry_observation: witness.post_registry_observation(),
            },
        )
        .unwrap();

        assert_eq!(
            super::require_pending_drain_durable_seal_match_v2(&witness, &mismatched),
            Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)
        );
        sealed.revalidate_sealed_v2().unwrap();
        assert_eq!(
            bootstrap.observe_recovery_empty_projection_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
        );
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
