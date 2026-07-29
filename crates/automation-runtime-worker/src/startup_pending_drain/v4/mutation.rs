use super::*;

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainActionIdentityV4 {
    pub(super) correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    pub(super) stage: RuntimePendingDrainActionStageV4,
}

impl RuntimePendingDrainActionIdentityV4 {
    pub(super) fn successor(
        selection: &RuntimeStartupRecoveryExecutionActionIdentityV2,
        stage: RuntimePendingDrainActionStageV4,
        offset: u64,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        if selection.class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
            return Err(RuntimePendingDrainV4Error::ActionClassMismatch);
        }
        let mut correlation = selection.correlation().clone();
        let revision = correlation
            .authority_revision()
            .get()
            .checked_add(offset)
            .filter(|value| *value <= i64::MAX as u64)
            .and_then(NonZeroU64::new)
            .ok_or(RuntimePendingDrainV4Error::AuthorityRevisionOverflow)?;
        correlation.replace_authority_revision_v4(revision);
        Ok(Self { correlation, stage })
    }

    pub fn correlation(&self) -> &RuntimeStartupRecoveryExecutionCorrelationV2 {
        &self.correlation
    }

    pub fn stage(&self) -> RuntimePendingDrainActionStageV4 {
        self.stage
    }
}

impl Debug for RuntimePendingDrainActionIdentityV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainActionIdentityV4(<redacted>)")
    }
}

trait RuntimePendingDrainCorrelationAdvanceV4 {
    fn replace_authority_revision_v4(&mut self, authority_revision: NonZeroU64);
}

impl RuntimePendingDrainCorrelationAdvanceV4 for RuntimeStartupRecoveryExecutionCorrelationV2 {
    fn replace_authority_revision_v4(&mut self, authority_revision: NonZeroU64) {
        self.replace_authority_revision_for_v4(authority_revision);
    }
}

pub(crate) struct RuntimeRoutedSealedWitnessInputV4 {
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub process_instance_id: ProcessInstanceId,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub slot: RuntimeServingSlotV2,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub admission_generation: NonZeroU64,
    pub route: RuntimeExactLocalRouteIdentityV2,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub active_guards: u64,
}

impl Debug for RuntimeRoutedSealedWitnessInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedSealedWitnessInputV4(<redacted>)")
    }
}

pub struct RuntimeRoutedSealedWitnessV4 {
    pub(super) registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) process_instance_id: ProcessInstanceId,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) slot: RuntimeServingSlotV2,
    pub(super) seal_key: [u8; 16],
    pub(super) seal_generation: NonZeroU64,
    pub(super) admission_generation: NonZeroU64,
    pub(super) route: RuntimeExactLocalRouteIdentityV2,
    pub(super) slot_observation_sequence: NonZeroU64,
    pub(super) registry_observation_sequence: NonZeroU64,
}

impl RuntimeRoutedSealedWitnessV4 {
    pub(crate) fn new(
        input: RuntimeRoutedSealedWitnessInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_registry_values(
            input.seal_generation,
            input.admission_generation,
            input.slot_observation_sequence,
            input.registry_observation_sequence,
        )?;
        if input.active_guards != 0 {
            return Err(RuntimePendingDrainV4Error::ActiveGuards);
        }
        if input.seal_key != input.intent_id.canonical_bytes()
            || input.route.slot() != input.slot
            || input.route.identity.process_instance_id != input.process_instance_id
        {
            return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
        }
        Ok(Self {
            registry_lifetime_digest: input.registry_lifetime_digest,
            process_instance_id: input.process_instance_id,
            intent_id: input.intent_id,
            slot: input.slot,
            seal_key: input.seal_key,
            seal_generation: input.seal_generation,
            admission_generation: input.admission_generation,
            route: input.route,
            slot_observation_sequence: input.slot_observation_sequence,
            registry_observation_sequence: input.registry_observation_sequence,
        })
    }

    pub fn route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.route
    }

    pub fn registry_lifetime_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.registry_lifetime_digest
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn seal_key(&self) -> &[u8; 16] {
        &self.seal_key
    }

    pub fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn admission_generation(&self) -> NonZeroU64 {
        self.admission_generation
    }

    pub fn slot_observation_sequence(&self) -> NonZeroU64 {
        self.slot_observation_sequence
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }
}

impl Debug for RuntimeRoutedSealedWitnessV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedSealedWitnessV4(<redacted>)")
    }
}

pub(crate) struct RuntimeRoutedClaimedSealedWitnessInputV4 {
    pub routed_seal: RuntimeRoutedSealedWitnessV4,
    pub claim_fence: FencingToken,
    pub claim_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl Debug for RuntimeRoutedClaimedSealedWitnessInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedClaimedSealedWitnessInputV4(<redacted>)")
    }
}

pub struct RuntimeRoutedClaimedSealedWitnessV4 {
    pub(super) routed_seal: RuntimeRoutedSealedWitnessV4,
    pub(super) claim_fence: FencingToken,
    pub(super) claim_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl RuntimeRoutedClaimedSealedWitnessV4 {
    pub(crate) fn new(
        input: RuntimeRoutedClaimedSealedWitnessInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        if input.routed_seal.route.controller_fencing_token.next().ok() != Some(input.claim_fence) {
            return Err(RuntimePendingDrainV4Error::ControllerFenceMismatch);
        }
        Ok(Self {
            routed_seal: input.routed_seal,
            claim_fence: input.claim_fence,
            claim_receipt_digest: input.claim_receipt_digest,
        })
    }
}

impl Debug for RuntimeRoutedClaimedSealedWitnessV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedClaimedSealedWitnessV4(<redacted>)")
    }
}

pub(crate) struct RuntimeLocallyRefencedSealedWitnessInputV4 {
    pub claimed: RuntimeRoutedClaimedSealedWitnessV4,
    pub old_route: RuntimeExactLocalRouteIdentityV2,
    pub removal_target: RuntimeExactLocalRouteIdentityV2,
    pub provenance: RuntimeRouteMutationProvenanceV2,
    pub registry_observation_sequence: NonZeroU64,
    pub refenced_at: DateTime<Utc>,
    pub active_guards: u64,
}

impl Debug for RuntimeLocallyRefencedSealedWitnessInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLocallyRefencedSealedWitnessInputV4(<redacted>)")
    }
}

pub struct RuntimeLocallyRefencedSealedWitnessV4 {
    pub(super) claimed: RuntimeRoutedClaimedSealedWitnessV4,
    pub(super) old_route: RuntimeExactLocalRouteIdentityV2,
    pub(super) removal_target: RuntimeExactLocalRouteIdentityV2,
    pub(super) provenance: RuntimeRouteMutationProvenanceV2,
    pub(super) registry_observation_sequence: NonZeroU64,
    pub(super) refenced_at: DateTime<Utc>,
}

impl RuntimeLocallyRefencedSealedWitnessV4 {
    pub(crate) fn new(
        input: RuntimeLocallyRefencedSealedWitnessInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_database_time(input.refenced_at)?;
        validate_persistence_value(input.registry_observation_sequence)?;
        if input.active_guards != 0 {
            return Err(RuntimePendingDrainV4Error::ActiveGuards);
        }
        if input.old_route != input.claimed.routed_seal.route
            || input.old_route.identity != input.removal_target.identity
            || input.old_route.route_incarnation != input.removal_target.route_incarnation
            || input.removal_target.controller_fencing_token != input.claimed.claim_fence
            || input.registry_observation_sequence
                <= input.claimed.routed_seal.registry_observation_sequence
        {
            return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
        }
        Ok(Self {
            claimed: input.claimed,
            old_route: input.old_route,
            removal_target: input.removal_target,
            provenance: input.provenance,
            registry_observation_sequence: input.registry_observation_sequence,
            refenced_at: input.refenced_at,
        })
    }

    pub fn refenced_at(&self) -> DateTime<Utc> {
        self.refenced_at
    }

    pub fn old_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.old_route
    }

    pub fn removal_target(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.removal_target
    }

    pub fn provenance(&self) -> &RuntimeRouteMutationProvenanceV2 {
        &self.provenance
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }

    pub fn claim_receipt_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.claimed.claim_receipt_digest
    }

    pub fn routed_seal(&self) -> &RuntimeRoutedSealedWitnessV4 {
        &self.claimed.routed_seal
    }
}

impl Debug for RuntimeLocallyRefencedSealedWitnessV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLocallyRefencedSealedWitnessV4(<redacted>)")
    }
}

pub(crate) struct RuntimeDurablyRefencedSealedWitnessInputV4 {
    pub locally_refenced: RuntimeLocallyRefencedSealedWitnessV4,
    pub refence_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl Debug for RuntimeDurablyRefencedSealedWitnessInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyRefencedSealedWitnessInputV4(<redacted>)")
    }
}

pub struct RuntimeDurablyRefencedSealedWitnessV4 {
    pub(super) locally_refenced: RuntimeLocallyRefencedSealedWitnessV4,
    pub(super) refence_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl RuntimeDurablyRefencedSealedWitnessV4 {
    pub(crate) fn new(input: RuntimeDurablyRefencedSealedWitnessInputV4) -> Self {
        Self {
            locally_refenced: input.locally_refenced,
            refence_receipt_digest: input.refence_receipt_digest,
        }
    }

    pub fn removal_target(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.locally_refenced.removal_target
    }

    pub fn refence_receipt_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.refence_receipt_digest
    }

    pub fn locally_refenced(&self) -> &RuntimeLocallyRefencedSealedWitnessV4 {
        &self.locally_refenced
    }
}

impl Debug for RuntimeDurablyRefencedSealedWitnessV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyRefencedSealedWitnessV4(<redacted>)")
    }
}

pub(crate) struct RuntimeRouteAbsentSealedWitnessInputV4 {
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub process_instance_id: ProcessInstanceId,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub slot: RuntimeServingSlotV2,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub admission_generation: NonZeroU64,
    pub source_route: RuntimeExactLocalRouteIdentityV2,
    pub removed_route: RuntimeExactLocalRouteIdentityV2,
    pub claim_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
    pub refence_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub active_guards: u64,
}

impl Debug for RuntimeRouteAbsentSealedWitnessInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteAbsentSealedWitnessInputV4(<redacted>)")
    }
}

pub struct RuntimeRouteAbsentSealedWitnessV4 {
    pub(super) registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) process_instance_id: ProcessInstanceId,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) slot: RuntimeServingSlotV2,
    pub(super) seal_key: [u8; 16],
    pub(super) seal_generation: NonZeroU64,
    pub(super) admission_generation: NonZeroU64,
    pub(super) source_route: RuntimeExactLocalRouteIdentityV2,
    pub(super) removed_route: RuntimeExactLocalRouteIdentityV2,
    pub(super) claim_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) refence_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) slot_observation_sequence: NonZeroU64,
    pub(super) registry_observation_sequence: NonZeroU64,
}

impl RuntimeRouteAbsentSealedWitnessV4 {
    pub(crate) fn new(
        input: RuntimeRouteAbsentSealedWitnessInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_registry_values(
            input.seal_generation,
            input.admission_generation,
            input.slot_observation_sequence,
            input.registry_observation_sequence,
        )?;
        if input.active_guards != 0 {
            return Err(RuntimePendingDrainV4Error::ActiveGuards);
        }
        if input.seal_key != input.intent_id.canonical_bytes()
            || input.source_route.slot() != input.slot
            || input.removed_route.slot() != input.slot
            || input.source_route.identity != input.removed_route.identity
            || input.source_route.route_incarnation != input.removed_route.route_incarnation
            || input.source_route.controller_fencing_token.next().ok()
                != Some(input.removed_route.controller_fencing_token)
            || input.process_instance_id != input.source_route.identity.process_instance_id
        {
            return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
        }
        Ok(Self {
            registry_lifetime_digest: input.registry_lifetime_digest,
            process_instance_id: input.process_instance_id,
            intent_id: input.intent_id,
            slot: input.slot,
            seal_key: input.seal_key,
            seal_generation: input.seal_generation,
            admission_generation: input.admission_generation,
            source_route: input.source_route,
            removed_route: input.removed_route,
            claim_receipt_digest: input.claim_receipt_digest,
            refence_receipt_digest: input.refence_receipt_digest,
            slot_observation_sequence: input.slot_observation_sequence,
            registry_observation_sequence: input.registry_observation_sequence,
        })
    }

    pub fn registry_lifetime_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.registry_lifetime_digest
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn seal_key(&self) -> &[u8; 16] {
        &self.seal_key
    }

    pub fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn admission_generation(&self) -> NonZeroU64 {
        self.admission_generation
    }

    pub fn slot_observation_sequence(&self) -> NonZeroU64 {
        self.slot_observation_sequence
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }

    pub fn source_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.source_route
    }

    pub fn removed_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.removed_route
    }

    pub fn claim_receipt_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.claim_receipt_digest
    }

    pub fn refence_receipt_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.refence_receipt_digest
    }
}

impl Debug for RuntimeRouteAbsentSealedWitnessV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteAbsentSealedWitnessV4(<redacted>)")
    }
}

pub(crate) struct RuntimeEmptySuccessionSealedWitnessInputV4 {
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub process_instance_id: ProcessInstanceId,
    pub successor_identity: RuntimeProcessIdentityV1,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub slot: RuntimeServingSlotV2,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub admission_generation: NonZeroU64,
    pub predecessor_route: RuntimeExactLocalRouteIdentityV2,
    pub possible_route_fence_ceiling: FencingToken,
    pub successor_fence: FencingToken,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub active_guards: u64,
}

impl Debug for RuntimeEmptySuccessionSealedWitnessInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptySuccessionSealedWitnessInputV4(<redacted>)")
    }
}

pub struct RuntimeEmptySuccessionSealedWitnessV4 {
    pub(super) registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) process_instance_id: ProcessInstanceId,
    pub(super) successor_identity: RuntimeProcessIdentityV1,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) slot: RuntimeServingSlotV2,
    pub(super) seal_key: [u8; 16],
    pub(super) seal_generation: NonZeroU64,
    pub(super) admission_generation: NonZeroU64,
    pub(super) predecessor_route: RuntimeExactLocalRouteIdentityV2,
    pub(super) possible_route_fence_ceiling: FencingToken,
    pub(super) successor_fence: FencingToken,
    pub(super) slot_observation_sequence: NonZeroU64,
    pub(super) registry_observation_sequence: NonZeroU64,
}

impl RuntimeEmptySuccessionSealedWitnessV4 {
    pub(crate) fn new(
        input: RuntimeEmptySuccessionSealedWitnessInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_registry_values(
            input.seal_generation,
            input.admission_generation,
            input.slot_observation_sequence,
            input.registry_observation_sequence,
        )?;
        if input.active_guards != 0 {
            return Err(RuntimePendingDrainV4Error::ActiveGuards);
        }
        if input.seal_key != input.intent_id.canonical_bytes()
            || input.predecessor_route.slot() != input.slot
            || input.predecessor_route.identity.process_instance_id == input.process_instance_id
            || input.successor_identity.process_instance_id != input.process_instance_id
            || input.successor_identity.target != input.predecessor_route.identity.target
            || input.possible_route_fence_ceiling.next().ok() != Some(input.successor_fence)
        {
            return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
        }
        Ok(Self {
            registry_lifetime_digest: input.registry_lifetime_digest,
            process_instance_id: input.process_instance_id,
            successor_identity: input.successor_identity,
            intent_id: input.intent_id,
            slot: input.slot,
            seal_key: input.seal_key,
            seal_generation: input.seal_generation,
            admission_generation: input.admission_generation,
            predecessor_route: input.predecessor_route,
            possible_route_fence_ceiling: input.possible_route_fence_ceiling,
            successor_fence: input.successor_fence,
            slot_observation_sequence: input.slot_observation_sequence,
            registry_observation_sequence: input.registry_observation_sequence,
        })
    }

    pub fn admission_generation(&self) -> NonZeroU64 {
        self.admission_generation
    }

    pub fn registry_lifetime_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.registry_lifetime_digest
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn seal_key(&self) -> &[u8; 16] {
        &self.seal_key
    }

    pub fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn predecessor_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.predecessor_route
    }

    pub fn possible_route_fence_ceiling(&self) -> FencingToken {
        self.possible_route_fence_ceiling
    }

    pub fn slot_observation_sequence(&self) -> NonZeroU64 {
        self.slot_observation_sequence
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }

    pub fn successor_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.successor_identity
    }

    pub fn successor_fence(&self) -> FencingToken {
        self.successor_fence
    }
}

impl Debug for RuntimeEmptySuccessionSealedWitnessV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptySuccessionSealedWitnessV4(<redacted>)")
    }
}

pub struct RuntimeAuthorizedRoutedDrainClaimV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) candidate: RuntimeUnclaimedPendingDrainCandidateV4,
    pub(super) seal: RuntimeRoutedSealedWitnessV4,
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
}

impl RuntimeAuthorizedRoutedDrainClaimV4 {
    pub fn candidate(&self) -> &RuntimeUnclaimedPendingDrainCandidateV4 {
        &self.candidate
    }

    #[cfg(test)]
    pub fn seal(&self) -> &RuntimeRoutedSealedWitnessV4 {
        &self.seal
    }

    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    fn validate_durable_receipt(
        &self,
        receipt: &RuntimeRoutedDrainClaimReceiptV4,
    ) -> Result<(), RuntimePendingDrainV4Error> {
        validate_mutation_source(
            &self.action_identity,
            &self.candidate.common,
            &receipt.mutation,
        )?;
        let result = receipt.result.canonical();
        let claim = result
            .intent()
            .state()
            .pending_claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        if result.intent().key() != self.candidate.common.canonical().intent().key()
            || result.intent().intent_revision() != receipt.mutation.result_intent_revision
            || result.state_bytes() != receipt.mutation.result_state_bytes.as_ref()
            || claim.gateway_owner_lease_id() != &self.candidate.current_owner().lease_id
            || claim.observed_owner_revision() != self.candidate.current_owner().owner_revision
            || claim.process_instance_id() != &self.seal.process_instance_id
            || claim.controller_fencing_token()
                != self
                    .candidate
                    .source_deployment_fence()
                    .next()
                    .map_err(|_| RuntimePendingDrainV4Error::ControllerFenceOverflow)?
            || claim.progress().seal().expected_route() != Some(&self.seal.route)
            || claim.progress().seal().seal_generation() != self.seal.seal_generation
            || claim.progress().seal().registry_observation_sequence()
                != self.seal.registry_observation_sequence
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        Ok(())
    }

    pub(crate) fn accept_durable_receipt(
        self,
        receipt: RuntimeRoutedDrainClaimReceiptV4,
    ) -> Result<RuntimeDurableRoutedClaimReceiptV4, RuntimePendingDrainV4Error> {
        self.validate_durable_receipt(&receipt)?;
        Ok(RuntimeDurableRoutedClaimReceiptV4 {
            authorization: self.authorization,
            source_common: self.candidate.common,
            source_seal: self.seal,
            receipt,
        })
    }

    pub(crate) fn authorize_determinate_non_commit_rollback(
        self,
        observation: RuntimeRoutedDrainDeterminateNonCommitObservationV4,
    ) -> Result<RuntimeRoutedDrainRollbackPermitV4, RuntimePendingDrainV4Error> {
        let canonical = observation.source.canonical();
        if observation.action_identity != self.action_identity
            || canonical.intent().key() != self.candidate.common.canonical().intent().key()
            || canonical.intent().intent_revision() != self.candidate.source_intent_revision()
            || canonical.state_bytes() != self.candidate.source_state_bytes()
            || observation.source_state_digest != *self.candidate.source_state_digest()
            || observation.owner != *self.candidate.current_owner()
            || observation.registry_lifetime_digest != self.seal.registry_lifetime_digest
            || observation.seal_generation != self.seal.seal_generation
            || observation.route != self.seal.route
            || observation.slot_observation_sequence != self.seal.slot_observation_sequence
            || observation.registry_observation_sequence < self.seal.registry_observation_sequence
        {
            return Err(RuntimePendingDrainV4Error::DeterminateNonCommitMismatch);
        }
        Ok(RuntimeRoutedDrainRollbackPermitV4 {
            action_identity: self.action_identity,
            seal: self.seal,
            observation_digest: observation.observation_digest,
            observed_at: observation.observed_at,
        })
    }
}

impl Debug for RuntimeAuthorizedRoutedDrainClaimV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedRoutedDrainClaimV4(<redacted>)")
    }
}

pub(super) enum RuntimeRefenceAuthorizationSourceV4 {
    Current(Box<RuntimeRoutedClaimedPendingDrainCandidateV4>),
    Applied(Box<RuntimeAppliedRoutedClaimSourceV4>),
}

pub struct RuntimeAuthorizedDrainRefenceProgressV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source: RuntimeRefenceAuthorizationSourceV4,
    pub(super) seal: RuntimeLocallyRefencedSealedWitnessV4,
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
}

impl RuntimeAuthorizedDrainRefenceProgressV4 {
    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    pub fn source_state_bytes(&self) -> &[u8] {
        match &self.source {
            RuntimeRefenceAuthorizationSourceV4::Current(candidate) => {
                candidate.source_state_bytes()
            }
            RuntimeRefenceAuthorizationSourceV4::Applied(receipt) => {
                receipt.receipt.result.canonical().state_bytes()
            }
        }
    }

    pub fn into_finalizer_registration(self) -> RuntimePendingDrainFinalizerRegistrationV4<Self> {
        RuntimePendingDrainFinalizerRegistrationV4::new(
            RuntimePendingDrainFinalizerIdentityV4::for_refence(&self),
            self,
        )
    }
}

impl RuntimeAuthorizedDrainRefenceProgressV4 {
    fn validate_durable_receipt(
        &self,
        receipt: &RuntimeDrainRefenceProgressReceiptV4,
    ) -> Result<(), RuntimePendingDrainV4Error> {
        let (common, source_claim) = match &self.source {
            RuntimeRefenceAuthorizationSourceV4::Current(candidate) => {
                validate_mutation_source(
                    &self.action_identity,
                    &candidate.common,
                    &receipt.mutation,
                )?;
                (&candidate.common, candidate.claim())
            }
            RuntimeRefenceAuthorizationSourceV4::Applied(applied) => {
                let claim = applied
                    .receipt
                    .result
                    .canonical()
                    .intent()
                    .state()
                    .pending_claim()
                    .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
                if receipt.mutation.action_identity != self.action_identity
                    || receipt.mutation.source_intent_revision
                        != applied
                            .receipt
                            .result
                            .canonical()
                            .intent()
                            .intent_revision()
                    || receipt.mutation.source_state_digest
                        != RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                            applied.receipt.result.canonical().state_bytes(),
                        )
                    || receipt.mutation.owner_receipt != applied.source_common.current_owner
                    || receipt.mutation.committed_at < applied.source_common.selection_database_now
                {
                    return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
                }
                (&applied.source_common, claim)
            }
        };
        let result = receipt.result.canonical();
        let claim = result
            .intent()
            .state()
            .pending_claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        if result.intent().key() != common.canonical().intent().key()
            || result.intent().intent_revision() != receipt.mutation.result_intent_revision
            || result.state_bytes() != receipt.mutation.result_state_bytes.as_ref()
            || claim.gateway_owner_lease_id() != source_claim.gateway_owner_lease_id()
            || claim.observed_owner_revision() != source_claim.observed_owner_revision()
            || claim.process_instance_id() != source_claim.process_instance_id()
            || claim.controller_fencing_token() != source_claim.controller_fencing_token()
            || claim.claim_epoch() != source_claim.claim_epoch()
            || claim.claim_revision().get()
                != source_claim
                    .claim_revision()
                    .get()
                    .checked_add(1)
                    .ok_or(RuntimePendingDrainV4Error::IntentRevisionOverflow)?
            || claim.progress().old_route() != Some(&self.seal.old_route)
            || claim.progress().removal_target() != Some(&self.seal.removal_target)
            || claim.progress().registry_observation_sequence()
                != Some(self.seal.registry_observation_sequence)
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        Ok(())
    }

    pub(crate) fn accept_durable_receipt(
        self,
        receipt: RuntimeDrainRefenceProgressReceiptV4,
    ) -> Result<RuntimeDurableRefenceReceiptV4, RuntimePendingDrainV4Error> {
        self.validate_durable_receipt(&receipt)?;
        Ok(RuntimeDurableRefenceReceiptV4 {
            authorization: self.authorization,
            source: self.source,
            local: self.seal,
            receipt,
        })
    }
}

impl Debug for RuntimeAuthorizedDrainRefenceProgressV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedDrainRefenceProgressV4(<redacted>)")
    }
}

pub(super) enum RuntimeAcknowledgementAuthorizationSourceV4 {
    Selected(Box<RuntimeRefencedPendingDrainCandidateV4>),
    Applied {
        common: Box<RuntimePendingDrainCandidateCommonV4>,
        claim_terminal_digest: RuntimePendingDrainEvidenceDigestV4,
        receipt: Box<RuntimeDrainRefenceProgressReceiptV4>,
    },
}

impl RuntimeAcknowledgementAuthorizationSourceV4 {
    pub(super) fn common(&self) -> &RuntimePendingDrainCandidateCommonV4 {
        match self {
            Self::Selected(candidate) => &candidate.common,
            Self::Applied { common, .. } => common,
        }
    }

    pub(super) fn canonical(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        match self {
            Self::Selected(candidate) => candidate.common.canonical(),
            Self::Applied { receipt, .. } => receipt.result.canonical(),
        }
    }
}

pub struct RuntimeAuthorizedSameProcessDrainAcknowledgementV4 {
    pub(super) _authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source: RuntimeAcknowledgementAuthorizationSourceV4,
    pub(super) route_absent: RuntimeRouteAbsentSealedWitnessV4,
    pub(super) certification: RuntimeDrainCertificationResolutionV2,
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
}

impl RuntimeAuthorizedSameProcessDrainAcknowledgementV4 {
    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    fn validate_durable_receipt(
        &self,
        receipt: &RuntimeSameProcessDrainAcknowledgementReceiptV4,
    ) -> Result<(), RuntimePendingDrainV4Error> {
        let (common, source_canonical) = match &self.source {
            RuntimeAcknowledgementAuthorizationSourceV4::Selected(candidate) => {
                validate_mutation_source(
                    &self.action_identity,
                    &candidate.common,
                    &receipt.mutation,
                )?;
                (&candidate.common, candidate.common.canonical())
            }
            RuntimeAcknowledgementAuthorizationSourceV4::Applied {
                common,
                claim_terminal_digest: _,
                receipt: refence,
            } => {
                if receipt.mutation.action_identity != self.action_identity
                    || receipt.mutation.source_intent_revision
                        != refence.result.canonical().intent().intent_revision()
                    || receipt.mutation.source_state_digest
                        != RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                            refence.result.canonical().state_bytes(),
                        )
                    || receipt.mutation.owner_receipt != common.current_owner
                {
                    return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
                }
                (common.as_ref(), refence.result.canonical())
            }
        };
        if receipt
            .result
            .state_kind()
            .map_err(|_| RuntimePendingDrainV4Error::MutationReceiptMismatch)?
            != RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
            || receipt.result.intent().key() != source_canonical.intent().key()
            || receipt.result.intent().intent_revision() != receipt.mutation.result_intent_revision
            || receipt.result.state_bytes() != receipt.mutation.result_state_bytes.as_ref()
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        let acknowledgement = receipt
            .result
            .intent()
            .state()
            .acknowledgement()
            .ok_or(RuntimePendingDrainV4Error::MutationReceiptMismatch)?;
        let source_claim = source_canonical
            .intent()
            .state()
            .pending_claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        if acknowledgement.claim() != source_claim
            || acknowledgement.expected_route() != Some(&self.route_absent.removed_route)
            || acknowledgement.registry_observation_sequence()
                != self.route_absent.registry_observation_sequence
            || acknowledgement.certification() != &self.certification
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        validate_resolution(common, &self.certification)?;
        Ok(())
    }

    pub(crate) fn accept_durable_receipt(
        self,
        receipt: RuntimeSameProcessDrainAcknowledgementReceiptV4,
    ) -> Result<RuntimeDurableSameProcessDrainAcknowledgementV4, RuntimePendingDrainV4Error> {
        self.validate_durable_receipt(&receipt)?;
        Ok(RuntimeDurableSameProcessDrainAcknowledgementV4 {
            source_intent_revision: self.source.canonical().intent().intent_revision(),
            source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                self.source.canonical().state_bytes(),
            ),
            intent_id: self.source.common().intent_id().clone(),
            route_absent: self.route_absent,
            receipt,
        })
    }
}

impl Debug for RuntimeAuthorizedSameProcessDrainAcknowledgementV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedSameProcessDrainAcknowledgementV4(<redacted>)")
    }
}

pub(super) enum RuntimePreviousProcessTeardownSourceV4 {
    RoutedClaimed(RuntimeRoutedClaimedPendingDrainCandidateV4),
    Refenced(RuntimeRefencedPendingDrainCandidateV4),
}

impl RuntimePreviousProcessTeardownSourceV4 {
    pub(super) fn common(&self) -> &RuntimePendingDrainCandidateCommonV4 {
        match self {
            Self::RoutedClaimed(candidate) => &candidate.common,
            Self::Refenced(candidate) => &candidate.common,
        }
    }

    fn predecessor_progress(&self) -> RuntimePreviousProcessDrainProgressV3 {
        match self {
            Self::RoutedClaimed(_) => RuntimePreviousProcessDrainProgressV3::RoutedClaimed,
            Self::Refenced(_) => RuntimePreviousProcessDrainProgressV3::Refenced,
        }
    }

    fn predecessor_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        match self {
            Self::RoutedClaimed(candidate) => candidate.source_route(),
            Self::Refenced(candidate) => candidate.source_route(),
        }
    }

    fn possible_route_fence_ceiling(&self) -> FencingToken {
        self.common()
            .claim()
            .expect("checked previous claim")
            .controller_fencing_token()
    }
}

pub struct RuntimeAuthorizedPreviousProcessDrainTeardownV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source: RuntimePreviousProcessTeardownSourceV4,
    pub(super) seal: RuntimeEmptySuccessionSealedWitnessV4,
    pub(super) certification: RuntimeDrainCertificationResolutionV2,
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
}

impl RuntimeAuthorizedPreviousProcessDrainTeardownV4 {
    #[cfg(test)]
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    fn validate_durable_receipt(
        &self,
        receipt: &RuntimePreviousProcessDrainTeardownReceiptV4,
    ) -> Result<(), RuntimePendingDrainV4Error> {
        let common = self.source.common();
        let predecessor_claim = common
            .claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        validate_mutation_source(&self.action_identity, common, &receipt.mutation)?;
        if receipt.result.state_kind()
            != RuntimeDrainIntentCanonicalStateKindV3::RouteAbsentAcknowledged
            || receipt.result.key() != common.canonical().intent().key()
            || receipt.result.drain_intent_digest()
                != common.canonical().intent().drain_intent_digest()
            || receipt.result.intent_revision() != receipt.mutation.result_intent_revision
            || receipt.result.state_bytes() != receipt.mutation.result_state_bytes.as_ref()
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        let acknowledgement = receipt
            .result
            .acknowledgement()
            .ok_or(RuntimePendingDrainV4Error::MutationReceiptMismatch)?;
        let successor_claim = acknowledgement.successor_claim();
        let successor_seal = successor_claim.progress().seal();
        let basis = acknowledgement.absence_basis();
        let claim_journal = common
            .claim_journal
            .as_ref()
            .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?;
        let expected_refence_digest = common
            .refence_journal
            .as_ref()
            .map(|journal| terminal_digest_v3(&journal.terminal_digest))
            .transpose()?;
        let expected_certification =
            RuntimePreviousProcessDrainCertificationResolutionV3::from_predecessor(
                common.canonical().intent().key(),
                predecessor_claim,
                self.certification.clone(),
            )
            .map_err(|_| RuntimePendingDrainV4Error::CertificationResolutionMismatch)?;
        let expected_claim_revision = predecessor_claim
            .claim_revision()
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(RuntimePendingDrainV4Error::IntentRevisionOverflow)?;
        let expected_provenance =
            expected_teardown_provenance(self.authorization.request(), &self.action_identity)?;
        if successor_claim.controller_fencing_token() != self.seal.successor_fence
            || successor_claim.process_instance_id() != &self.seal.process_instance_id
            || successor_claim.gateway_owner_lease_id() != &common.current_owner.lease_id
            || successor_claim.observed_owner_revision() != common.current_owner.owner_revision
            || successor_claim.claim_revision() != expected_claim_revision
            || successor_claim.expires_at() != common.current_owner.expires_at
            || successor_claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed
            || successor_seal.expected_route().is_some()
            || successor_seal.intent_id() != common.intent_id()
            || successor_seal.slot() != common.slot()
            || successor_seal.process_instance_id() != &self.seal.process_instance_id
            || successor_seal.seal_generation() != self.seal.seal_generation
            || successor_seal.registry_observation_sequence() != self.seal.slot_observation_sequence
            || basis.predecessor_intent_revision() != common.canonical().intent().intent_revision()
            || basis.predecessor_state_digest() != &common.source_state_digest
            || basis.predecessor_progress() != self.source.predecessor_progress()
            || basis.route_identity() != &self.seal.predecessor_route.identity
            || basis.route_identity() != &self.source.predecessor_route().identity
            || basis.route_incarnation() != self.seal.predecessor_route.route_incarnation
            || basis.route_incarnation() != self.source.predecessor_route().route_incarnation
            || basis.source_route_fence()
                != self.source.predecessor_route().controller_fencing_token
            || basis.possible_route_fence_ceiling() != self.source.possible_route_fence_ceiling()
            || basis.possible_route_fence_ceiling() != self.seal.possible_route_fence_ceiling
            || basis.predecessor_claim_terminal_digest()
                != &terminal_digest_v3(&claim_journal.terminal_digest)?
            || basis.predecessor_refence_terminal_digest() != expected_refence_digest.as_ref()
            || acknowledgement.provenance() != &expected_provenance
            || acknowledgement.registry_observation_sequence()
                != self.seal.registry_observation_sequence
            || acknowledgement.certification() != &expected_certification
            || acknowledgement.acknowledged_at() != receipt.mutation.committed_at
            || successor_claim.claim_epoch()
                != match &expected_provenance {
                    RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => {
                        witness.recovery_generation
                    }
                    RuntimeRouteMutationProvenanceV2::Ordinary { .. }
                    | RuntimeRouteMutationProvenanceV2::Shutdown(_) => {
                        return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
                    }
                }
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        Ok(())
    }

    pub(crate) fn accept_durable_receipt(
        self,
        receipt: RuntimePreviousProcessDrainTeardownReceiptV4,
    ) -> Result<RuntimeDurablePreviousProcessDrainTeardownV4, RuntimePendingDrainV4Error> {
        self.validate_durable_receipt(&receipt)?;
        Ok(RuntimeDurablePreviousProcessDrainTeardownV4 {
            source_intent_revision: self.source.common().canonical().intent().intent_revision(),
            source_state_digest: self.source.common().source_state_digest.clone(),
            intent_id: self.source.common().intent_id().clone(),
            seal: self.seal,
            receipt,
        })
    }
}

impl Debug for RuntimeAuthorizedPreviousProcessDrainTeardownV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPreviousProcessDrainTeardownV4(<redacted>)")
    }
}

pub(crate) struct RuntimeRoutedDrainDeterminateNonCommitObservationInputV4 {
    pub action_identity: RuntimePendingDrainActionIdentityV4,
    pub source: RuntimePersistedUnclaimedPendingDrainIntentV2,
    pub source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub owner: RuntimeGatewayOwnerLeaseReceiptV1,
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub seal_generation: NonZeroU64,
    pub route: RuntimeExactLocalRouteIdentityV2,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub observation_digest: RuntimePendingDrainEvidenceDigestV4,
    pub observed_at: DateTime<Utc>,
}

impl Debug for RuntimeRoutedDrainDeterminateNonCommitObservationInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainDeterminateNonCommitObservationInputV4(<redacted>)")
    }
}

pub struct RuntimeRoutedDrainDeterminateNonCommitObservationV4 {
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
    pub(super) source: RuntimePersistedUnclaimedPendingDrainIntentV2,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) owner: RuntimeGatewayOwnerLeaseReceiptV1,
    pub(super) registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) seal_generation: NonZeroU64,
    pub(super) route: RuntimeExactLocalRouteIdentityV2,
    pub(super) slot_observation_sequence: NonZeroU64,
    pub(super) registry_observation_sequence: NonZeroU64,
    pub(super) observation_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) observed_at: DateTime<Utc>,
}

impl RuntimeRoutedDrainDeterminateNonCommitObservationV4 {
    pub(crate) fn new(
        input: RuntimeRoutedDrainDeterminateNonCommitObservationInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_database_time(input.observed_at)?;
        let canonical = input.source.canonical();
        if RuntimeDrainCanonicalStateDigestV3::from_state_bytes(canonical.state_bytes())
            != input.source_state_digest
            || input.route.slot() != canonical.intent().key().slot
            || input.route.identity.target != canonical.intent().key().expected_target
        {
            return Err(RuntimePendingDrainV4Error::DeterminateNonCommitMismatch);
        }
        validate_owner_current(&input.owner)?;
        validate_persistence_value(input.seal_generation)?;
        validate_persistence_value(input.slot_observation_sequence)?;
        validate_persistence_value(input.registry_observation_sequence)?;
        if input.observed_at < input.owner.database_now {
            return Err(RuntimePendingDrainV4Error::DatabaseClockRegressed);
        }
        Ok(Self {
            action_identity: input.action_identity,
            source: input.source,
            source_state_digest: input.source_state_digest,
            owner: input.owner,
            registry_lifetime_digest: input.registry_lifetime_digest,
            seal_generation: input.seal_generation,
            route: input.route,
            slot_observation_sequence: input.slot_observation_sequence,
            registry_observation_sequence: input.registry_observation_sequence,
            observation_digest: input.observation_digest,
            observed_at: input.observed_at,
        })
    }
}

impl Debug for RuntimeRoutedDrainDeterminateNonCommitObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainDeterminateNonCommitObservationV4(<redacted>)")
    }
}

pub struct RuntimeRoutedDrainRollbackPermitV4 {
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
    pub(super) seal: RuntimeRoutedSealedWitnessV4,
    pub(super) observation_digest: RuntimePendingDrainEvidenceDigestV4,
    pub(super) observed_at: DateTime<Utc>,
}

impl RuntimeRoutedDrainRollbackPermitV4 {
    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    pub fn seal(&self) -> &RuntimeRoutedSealedWitnessV4 {
        &self.seal
    }

    pub fn observation_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.observation_digest
    }

    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }
}

impl Debug for RuntimeRoutedDrainRollbackPermitV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainRollbackPermitV4(<redacted>)")
    }
}

pub(crate) struct RuntimePendingDrainMutationReceiptInputV4 {
    pub action_identity: RuntimePendingDrainActionIdentityV4,
    pub source_intent_revision: NonZeroU64,
    pub source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub result_intent_revision: NonZeroU64,
    pub result_state_bytes: Box<[u8]>,
    pub result_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub committed_at: DateTime<Utc>,
}

impl Debug for RuntimePendingDrainMutationReceiptInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainMutationReceiptInputV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainMutationReceiptV4 {
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
    pub(super) source_intent_revision: NonZeroU64,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) result_intent_revision: NonZeroU64,
    pub(super) result_state_bytes: Box<[u8]>,
    pub(super) result_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub(super) terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub(super) committed_at: DateTime<Utc>,
}

impl RuntimePendingDrainMutationReceiptV4 {
    pub(crate) fn new(
        input: RuntimePendingDrainMutationReceiptInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_database_time(input.committed_at)?;
        validate_persistence_value(input.source_intent_revision)?;
        validate_persistence_value(input.result_intent_revision)?;
        if input
            .source_intent_revision
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            != Some(input.result_intent_revision)
            || RuntimeDrainCanonicalStateDigestV3::from_state_bytes(&input.result_state_bytes)
                != input.result_state_digest
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        validate_owner_current(&input.owner_receipt)?;
        if input.committed_at < input.owner_receipt.database_now {
            return Err(RuntimePendingDrainV4Error::DatabaseClockRegressed);
        }
        Ok(Self {
            action_identity: input.action_identity,
            source_intent_revision: input.source_intent_revision,
            source_state_digest: input.source_state_digest,
            result_intent_revision: input.result_intent_revision,
            result_state_bytes: input.result_state_bytes,
            result_state_digest: input.result_state_digest,
            owner_receipt: input.owner_receipt,
            terminal_digest: input.terminal_digest,
            committed_at: input.committed_at,
        })
    }

    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    pub fn result_state_bytes(&self) -> &[u8] {
        &self.result_state_bytes
    }

    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.terminal_digest
    }
}

impl Debug for RuntimePendingDrainMutationReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainMutationReceiptV4(<redacted>)")
    }
}

pub struct RuntimeRoutedDrainClaimReceiptV4 {
    pub(super) mutation: RuntimePendingDrainMutationReceiptV4,
    pub(super) result: RuntimePersistedRoutedClaimedPendingDrainIntentV2,
}

impl RuntimeRoutedDrainClaimReceiptV4 {
    pub(crate) fn new(
        mutation: RuntimePendingDrainMutationReceiptV4,
        result: RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_typed_v2_result(result.canonical(), &mutation)?;
        Ok(Self { mutation, result })
    }
}

impl Debug for RuntimeRoutedDrainClaimReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainClaimReceiptV4(<redacted>)")
    }
}

pub struct RuntimeDurableRoutedClaimReceiptV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source_common: RuntimePendingDrainCandidateCommonV4,
    pub(super) source_seal: RuntimeRoutedSealedWitnessV4,
    pub(super) receipt: RuntimeRoutedDrainClaimReceiptV4,
}

impl RuntimeDurableRoutedClaimReceiptV4 {
    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        self.receipt.mutation.terminal_digest()
    }

    pub fn result(&self) -> &RuntimePersistedRoutedClaimedPendingDrainIntentV2 {
        &self.receipt.result
    }

    pub fn claim_fence(&self) -> FencingToken {
        self.receipt
            .result
            .canonical()
            .intent()
            .state()
            .pending_claim()
            .expect("checked routed claim receipt")
            .controller_fencing_token()
    }

    pub fn source_seal(&self) -> &RuntimeRoutedSealedWitnessV4 {
        &self.source_seal
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_common.canonical().intent().intent_revision()
    }

    pub fn source_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.source_common.source_state_digest
    }

    pub(super) fn bind_registry_claim(
        self,
        witness: RuntimeRoutedClaimedSealedWitnessV4,
    ) -> Result<RuntimeDurablyRoutedClaimedV4, RuntimePendingDrainV4Error> {
        if witness.routed_seal.registry_lifetime_digest != self.source_seal.registry_lifetime_digest
            || witness.routed_seal.intent_id != self.source_seal.intent_id
            || witness.routed_seal.seal_generation != self.source_seal.seal_generation
            || witness.claim_receipt_digest.as_bytes()
                != self.receipt.mutation.terminal_digest.as_bytes()
        {
            return Err(RuntimePendingDrainV4Error::RegistryReceiptMismatch);
        }
        Ok(RuntimeDurablyRoutedClaimedV4 {
            authorization: self.authorization,
            source_common: self.source_common,
            claim_receipt: self.receipt,
            witness,
        })
    }
}

impl Debug for RuntimeDurableRoutedClaimReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableRoutedClaimReceiptV4(<redacted>)")
    }
}

pub struct RuntimeDurablyRoutedClaimedV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source_common: RuntimePendingDrainCandidateCommonV4,
    pub(super) claim_receipt: RuntimeRoutedDrainClaimReceiptV4,
    pub(super) witness: RuntimeRoutedClaimedSealedWitnessV4,
}

impl Debug for RuntimeDurablyRoutedClaimedV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyRoutedClaimedV4(<redacted>)")
    }
}

pub(super) struct RuntimeAppliedRoutedClaimSourceV4 {
    pub(super) source_common: RuntimePendingDrainCandidateCommonV4,
    pub(super) receipt: RuntimeRoutedDrainClaimReceiptV4,
}

pub struct RuntimeDrainRefenceProgressReceiptV4 {
    pub(super) mutation: RuntimePendingDrainMutationReceiptV4,
    pub(super) result: RuntimePersistedRefencedPendingDrainIntentV2,
}

impl RuntimeDrainRefenceProgressReceiptV4 {
    pub(crate) fn new(
        mutation: RuntimePendingDrainMutationReceiptV4,
        result: RuntimePersistedRefencedPendingDrainIntentV2,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_typed_v2_result(result.canonical(), &mutation)?;
        Ok(Self { mutation, result })
    }
}

impl Debug for RuntimeDrainRefenceProgressReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDrainRefenceProgressReceiptV4(<redacted>)")
    }
}

pub struct RuntimeDurableRefenceReceiptV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source: RuntimeRefenceAuthorizationSourceV4,
    pub(super) local: RuntimeLocallyRefencedSealedWitnessV4,
    pub(super) receipt: RuntimeDrainRefenceProgressReceiptV4,
}

impl RuntimeDurableRefenceReceiptV4 {
    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        self.receipt.mutation.terminal_digest()
    }

    pub fn result(&self) -> &RuntimePersistedRefencedPendingDrainIntentV2 {
        &self.receipt.result
    }

    pub fn claim_receipt_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.local.claimed.claim_receipt_digest
    }

    pub fn refence_receipt_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        RuntimePendingDrainEvidenceDigestV4::new(*self.receipt.mutation.terminal_digest.as_bytes())
            .expect("checked terminal digest is nonzero")
    }

    pub fn source_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.local.old_route
    }

    pub fn removal_target(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        &self.local.removal_target
    }

    pub fn source_seal(&self) -> &RuntimeRoutedSealedWitnessV4 {
        &self.local.claimed.routed_seal
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.local.registry_observation_sequence
    }

    pub(super) fn bind_registry_refence(
        self,
        durable: RuntimeDurablyRefencedSealedWitnessV4,
    ) -> Result<RuntimeDurablyRefencedDrainV4, RuntimePendingDrainV4Error> {
        if durable
            .locally_refenced
            .claimed
            .routed_seal
            .registry_lifetime_digest
            != self.local.claimed.routed_seal.registry_lifetime_digest
            || durable.locally_refenced.claimed.routed_seal.intent_id
                != self.local.claimed.routed_seal.intent_id
            || durable.locally_refenced.removal_target != self.local.removal_target
            || durable.refence_receipt_digest.as_bytes()
                != self.receipt.mutation.terminal_digest.as_bytes()
        {
            return Err(RuntimePendingDrainV4Error::RegistryReceiptMismatch);
        }
        let (common, claim_terminal_digest) = match self.source {
            RuntimeRefenceAuthorizationSourceV4::Current(candidate) => {
                let candidate = *candidate;
                let digest = *candidate
                    .common
                    .claim_journal
                    .as_ref()
                    .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?
                    .terminal_digest
                    .as_bytes();
                (
                    Box::new(candidate.common),
                    RuntimePendingDrainEvidenceDigestV4::new(digest)?,
                )
            }
            RuntimeRefenceAuthorizationSourceV4::Applied(applied) => {
                let applied = *applied;
                (
                    Box::new(applied.source_common),
                    RuntimePendingDrainEvidenceDigestV4::new(
                        *applied.receipt.mutation.terminal_digest.as_bytes(),
                    )?,
                )
            }
        };
        Ok(RuntimeDurablyRefencedDrainV4 {
            authorization: self.authorization,
            source: RuntimeAcknowledgementAuthorizationSourceV4::Applied {
                common,
                claim_terminal_digest,
                receipt: Box::new(self.receipt),
            },
            durable,
        })
    }
}

impl Debug for RuntimeDurableRefenceReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableRefenceReceiptV4(<redacted>)")
    }
}

pub struct RuntimeDurablyRefencedDrainV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) source: RuntimeAcknowledgementAuthorizationSourceV4,
    pub(super) durable: RuntimeDurablyRefencedSealedWitnessV4,
}

impl RuntimeDurablyRefencedDrainV4 {
    pub fn durable_witness(&self) -> &RuntimeDurablyRefencedSealedWitnessV4 {
        &self.durable
    }

    pub(super) fn bind_route_absent(
        self,
        route_absent: RuntimeRouteAbsentSealedWitnessV4,
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> Result<RuntimeAuthorizedSameProcessDrainAcknowledgementV4, RuntimePendingDrainV4Error>
    {
        let common = match &self.source {
            RuntimeAcknowledgementAuthorizationSourceV4::Selected(candidate) => &candidate.common,
            RuntimeAcknowledgementAuthorizationSourceV4::Applied { common, .. } => common,
        };
        match &self.source {
            RuntimeAcknowledgementAuthorizationSourceV4::Selected(_) => {
                validate_route_absent(common, &route_absent)?;
            }
            RuntimeAcknowledgementAuthorizationSourceV4::Applied {
                claim_terminal_digest,
                receipt,
                ..
            } => {
                let claim = receipt
                    .result
                    .canonical()
                    .intent()
                    .state()
                    .pending_claim()
                    .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
                validate_route_absent_for_claim(common, claim, &route_absent)?;
                if route_absent.claim_receipt_digest != *claim_terminal_digest
                    || route_absent.refence_receipt_digest.as_bytes()
                        != receipt.mutation.terminal_digest.as_bytes()
                {
                    return Err(RuntimePendingDrainV4Error::RegistryReceiptMismatch);
                }
            }
        }
        validate_resolution(common, &certification)?;
        validate_resolved_serving(common, &certification)?;
        if self.durable.locally_refenced.old_route != route_absent.source_route
            || self.durable.locally_refenced.removal_target != route_absent.removed_route
            || self.durable.refence_receipt_digest != route_absent.refence_receipt_digest
        {
            return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
        }
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            self.authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::SameProcessAcknowledgement,
            3,
        )?;
        Ok(RuntimeAuthorizedSameProcessDrainAcknowledgementV4 {
            _authorization: self.authorization,
            source: self.source,
            route_absent,
            certification,
            action_identity,
        })
    }
}

impl Debug for RuntimeDurablyRefencedDrainV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyRefencedDrainV4(<redacted>)")
    }
}

pub struct RuntimeSameProcessDrainAcknowledgementReceiptV4 {
    pub(super) mutation: RuntimePendingDrainMutationReceiptV4,
    pub(super) result: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimeSameProcessDrainAcknowledgementReceiptV4 {
    pub(crate) fn new(
        mutation: RuntimePendingDrainMutationReceiptV4,
        result: RuntimeCanonicalDrainIntentStateV2,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_typed_v2_result(&result, &mutation)?;
        if result
            .state_kind()
            .map_err(|_| RuntimePendingDrainV4Error::MutationReceiptMismatch)?
            != RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        Ok(Self { mutation, result })
    }
}

impl Debug for RuntimeSameProcessDrainAcknowledgementReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSameProcessDrainAcknowledgementReceiptV4(<redacted>)")
    }
}

pub struct RuntimeDurableSameProcessDrainAcknowledgementV4 {
    pub(super) source_intent_revision: NonZeroU64,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) route_absent: RuntimeRouteAbsentSealedWitnessV4,
    pub(super) receipt: RuntimeSameProcessDrainAcknowledgementReceiptV4,
}

impl RuntimeDurableSameProcessDrainAcknowledgementV4 {
    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        self.receipt.mutation.terminal_digest()
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.receipt.result
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn source_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.source_state_digest
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn route_absent_witness(&self) -> &RuntimeRouteAbsentSealedWitnessV4 {
        &self.route_absent
    }
}

impl Debug for RuntimeDurableSameProcessDrainAcknowledgementV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableSameProcessDrainAcknowledgementV4(<redacted>)")
    }
}

pub struct RuntimePreviousProcessDrainTeardownReceiptV4 {
    pub(super) mutation: RuntimePendingDrainMutationReceiptV4,
    pub(super) result: RuntimeCanonicalDrainIntentStateV3,
}

impl RuntimePreviousProcessDrainTeardownReceiptV4 {
    pub(crate) fn new(
        mutation: RuntimePendingDrainMutationReceiptV4,
        result: RuntimeCanonicalDrainIntentStateV3,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        if result.intent_revision() != mutation.result_intent_revision
            || result.state_bytes() != mutation.result_state_bytes.as_ref()
            || RuntimeDrainCanonicalStateDigestV3::from_state_bytes(result.state_bytes())
                != mutation.result_state_digest
            || result.state_kind()
                != RuntimeDrainIntentCanonicalStateKindV3::RouteAbsentAcknowledged
        {
            return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
        }
        Ok(Self { mutation, result })
    }
}

impl Debug for RuntimePreviousProcessDrainTeardownReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreviousProcessDrainTeardownReceiptV4(<redacted>)")
    }
}

pub struct RuntimeDurablePreviousProcessDrainTeardownV4 {
    pub(super) source_intent_revision: NonZeroU64,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) seal: RuntimeEmptySuccessionSealedWitnessV4,
    pub(super) receipt: RuntimePreviousProcessDrainTeardownReceiptV4,
}

impl RuntimeDurablePreviousProcessDrainTeardownV4 {
    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        self.receipt.mutation.terminal_digest()
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV3 {
        &self.receipt.result
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn source_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.source_state_digest
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn successor_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.seal.successor_identity
    }

    pub fn successor_fence(&self) -> FencingToken {
        self.seal.successor_fence
    }

    pub fn seal(&self) -> &RuntimeEmptySuccessionSealedWitnessV4 {
        &self.seal
    }
}

impl Debug for RuntimeDurablePreviousProcessDrainTeardownV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablePreviousProcessDrainTeardownV4(<redacted>)")
    }
}
