use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::Instant;

use automation_runtime_controller::{
    RuntimeDrainIntentIdV2, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeServingSlotV2,
};
use automation_runtime_convergence::{ProcessInstanceId, RuntimeDeploymentTargetV1};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeCompletedStartupRecoveryExecutionV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryEmptyObservationV2,
    RuntimeRegistryRecoveryObservationInputV2, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryExecutionActionIdentityV2, RuntimeStartupRecoveryExecutionCorrelationV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2, RuntimeStartupRecoveryExecutionReceiptV2,
    RuntimeStartupRecoveryExecutionTerminalDigestV2,
};

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainStateDigestV2([u8; 32]);

impl RuntimePendingDrainStateDigestV2 {
    pub fn new(value: [u8; 32]) -> Result<Self, RuntimePendingDrainCompoundErrorV2> {
        if value == [0; 32] {
            return Err(RuntimePendingDrainCompoundErrorV2::ZeroDigest);
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for RuntimePendingDrainStateDigestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainStateDigestV2(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainCandidateV2 {
    intent_id: RuntimeDrainIntentIdV2,
    slot: RuntimeServingSlotV2,
    expected_target: RuntimeDeploymentTargetV1,
    source_intent_revision: NonZeroU64,
    source_state_digest: RuntimePendingDrainStateDigestV2,
}

impl RuntimePendingDrainCandidateV2 {
    pub fn new(
        intent_id: RuntimeDrainIntentIdV2,
        slot: RuntimeServingSlotV2,
        expected_target: RuntimeDeploymentTargetV1,
        source_intent_revision: NonZeroU64,
        source_state_digest: RuntimePendingDrainStateDigestV2,
    ) -> Result<Self, RuntimePendingDrainCompoundErrorV2> {
        if !slot.matches_target(&expected_target) {
            return Err(RuntimePendingDrainCompoundErrorV2::CandidateTargetMismatch);
        }
        if source_intent_revision
            .get()
            .checked_add(2)
            .is_none_or(|revision| revision > i64::MAX as u64)
        {
            return Err(RuntimePendingDrainCompoundErrorV2::IntentRevisionOverflow);
        }
        Ok(Self {
            intent_id,
            slot,
            expected_target,
            source_intent_revision,
            source_state_digest,
        })
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn expected_target(&self) -> &RuntimeDeploymentTargetV1 {
        &self.expected_target
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn source_state_digest(&self) -> &RuntimePendingDrainStateDigestV2 {
        &self.source_state_digest
    }
}

impl Debug for RuntimePendingDrainCandidateV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainCandidateV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimePendingDrainSlotObservationV2 {
    pub admission_generation: NonZeroU64,
    pub observation_sequence: NonZeroU64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainRegistrySealWitnessInputV2 {
    pub process_instance_id: ProcessInstanceId,
    pub slot: RuntimeServingSlotV2,
    pub pre_slot_observation: Option<RuntimePendingDrainSlotObservationV2>,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub post_slot_admission_generation: NonZeroU64,
    pub post_slot_observation_sequence: NonZeroU64,
    pub pre_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    pub pre_registry_retained_slot_count: u64,
    pub pre_registry_retained_empty_tombstone_count: u64,
    pub post_registry_observation: RuntimeRegistryRecoveryObservationInputV2,
}

impl Debug for RuntimePendingDrainRegistrySealWitnessInputV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainRegistrySealWitnessInputV2(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainRegistrySealWitnessV2 {
    process_instance_id: ProcessInstanceId,
    slot: RuntimeServingSlotV2,
    pre_slot_observation: Option<RuntimePendingDrainSlotObservationV2>,
    seal_key: [u8; 16],
    seal_generation: NonZeroU64,
    post_slot_admission_generation: NonZeroU64,
    post_slot_observation_sequence: NonZeroU64,
    pre_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    pre_registry_retained_slot_count: u64,
    pre_registry_retained_empty_tombstone_count: u64,
    post_registry_observation: RuntimeRegistryRecoveryObservationInputV2,
}

impl RuntimePendingDrainRegistrySealWitnessV2 {
    pub fn new(
        input: RuntimePendingDrainRegistrySealWitnessInputV2,
    ) -> Result<Self, RuntimePendingDrainCompoundErrorV2> {
        validate_persistence_value(input.seal_generation.get())?;
        validate_persistence_value(input.post_slot_admission_generation.get())?;
        validate_persistence_value(input.post_slot_observation_sequence.get())?;
        validate_persistence_value(input.pre_registry_observation_sequence.get())?;
        validate_persistence_count(input.pre_registry_retained_slot_count)?;
        validate_persistence_count(input.pre_registry_retained_empty_tombstone_count)?;
        validate_registry_seal_transition_v2(&input)?;
        Ok(Self {
            process_instance_id: input.process_instance_id,
            slot: input.slot,
            pre_slot_observation: input.pre_slot_observation,
            seal_key: input.seal_key,
            seal_generation: input.seal_generation,
            post_slot_admission_generation: input.post_slot_admission_generation,
            post_slot_observation_sequence: input.post_slot_observation_sequence,
            pre_registry_observation_sequence: input.pre_registry_observation_sequence,
            pre_registry_retained_slot_count: input.pre_registry_retained_slot_count,
            pre_registry_retained_empty_tombstone_count: input
                .pre_registry_retained_empty_tombstone_count,
            post_registry_observation: input.post_registry_observation,
        })
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn pre_slot_observation(&self) -> Option<RuntimePendingDrainSlotObservationV2> {
        self.pre_slot_observation
    }

    pub fn seal_key(&self) -> &[u8; 16] {
        &self.seal_key
    }

    pub fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn post_slot_admission_generation(&self) -> NonZeroU64 {
        self.post_slot_admission_generation
    }

    pub fn post_slot_observation_sequence(&self) -> NonZeroU64 {
        self.post_slot_observation_sequence
    }

    pub fn pre_registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.pre_registry_observation_sequence
    }

    pub fn pre_registry_retained_slot_count(&self) -> u64 {
        self.pre_registry_retained_slot_count
    }

    pub fn pre_registry_retained_empty_tombstone_count(&self) -> u64 {
        self.pre_registry_retained_empty_tombstone_count
    }

    pub fn post_registry_observation(&self) -> RuntimeRegistryRecoveryObservationInputV2 {
        self.post_registry_observation
    }
}

impl Debug for RuntimePendingDrainRegistrySealWitnessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainRegistrySealWitnessV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainSelectionReceiptV2 {
    correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    outcome: RuntimePendingDrainSelectionOutcomeV2,
}

impl RuntimePendingDrainSelectionReceiptV2 {
    pub fn new(
        correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        outcome: RuntimePendingDrainSelectionOutcomeV2,
    ) -> Self {
        Self {
            correlation,
            owner_receipt,
            outcome,
        }
    }
}

impl Debug for RuntimePendingDrainSelectionReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainSelectionReceiptV2(<redacted>)")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimePendingDrainSelectionOutcomeV2 {
    Candidate(RuntimePendingDrainCandidateV2),
    NoCandidate,
}

pub trait RuntimePendingDrainSelectionPortV2 {
    type Error;

    fn select_pending_drain(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainSelectionReceiptV2, Self::Error>> + Send;
}

pub struct RuntimeAuthorizedPendingDrainSelectionV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    acknowledgement_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
}

impl RuntimeAuthorizedPendingDrainSelectionV2 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn claim_action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        self.authorization.request().action_identity()
    }

    pub fn acknowledgement_action_identity(
        &self,
    ) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.acknowledgement_action_identity
    }

    pub fn accept_selection(
        self,
        receipt: RuntimePendingDrainSelectionReceiptV2,
    ) -> Result<RuntimeAcceptedPendingDrainSelectionV2, RuntimePendingDrainCompoundErrorV2> {
        if receipt.correlation != *self.authorization.request().correlation() {
            return Err(RuntimePendingDrainCompoundErrorV2::CorrelationMismatch);
        }
        validate_owner_receipt_v2(
            self.authorization.request(),
            &receipt.owner_receipt,
            self.authorization.request().minimum_database_now(),
        )?;
        match receipt.outcome {
            RuntimePendingDrainSelectionOutcomeV2::Candidate(candidate) => {
                Ok(RuntimeAcceptedPendingDrainSelectionV2::Candidate(Box::new(
                    RuntimeSelectedPendingDrainCandidateV2 {
                        authorization: self.authorization,
                        acknowledgement_action_identity: self.acknowledgement_action_identity,
                        candidate,
                        selection_owner_receipt: receipt.owner_receipt,
                    },
                )))
            }
            RuntimePendingDrainSelectionOutcomeV2::NoCandidate => {
                Ok(RuntimeAcceptedPendingDrainSelectionV2::NoCandidate(
                    Box::new(RuntimeSelectedPendingDrainNoCandidateV2 {
                        authorization: self.authorization,
                        selection_owner_receipt: receipt.owner_receipt,
                    }),
                ))
            }
        }
    }
}

impl Debug for RuntimeAuthorizedPendingDrainSelectionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPendingDrainSelectionV2(<redacted>)")
    }
}

pub enum RuntimeAcceptedPendingDrainSelectionV2 {
    Candidate(Box<RuntimeSelectedPendingDrainCandidateV2>),
    NoCandidate(Box<RuntimeSelectedPendingDrainNoCandidateV2>),
}

pub enum RuntimePendingDrainExecutionProofV2 {
    NoCandidate(RuntimePendingDrainNoCandidateProofV2),
    Compound(Box<RuntimePendingDrainCompoundProofV2>),
}

impl Debug for RuntimePendingDrainExecutionProofV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainExecutionProofV2(<redacted>)")
    }
}

impl RuntimePendingDrainExecutionProofV2 {
    pub(crate) fn matches_request(
        &self,
        request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    ) -> bool {
        if request.class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
            return false;
        }
        match self {
            Self::NoCandidate(proof) => proof.action_identity == *request.action_identity(),
            Self::Compound(proof) => {
                proof.claim_action_identity == *request.action_identity()
                    && request
                        .action_identity()
                        .pending_drain_acknowledgement_successor()
                        .is_some_and(|identity| identity == proof.acknowledgement_action_identity)
            }
        }
    }

    pub(crate) fn requires_registry_successor(&self) -> bool {
        matches!(self, Self::Compound(_))
    }
}

pub struct RuntimePendingDrainNoCandidateProofV2 {
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
}

impl RuntimePendingDrainNoCandidateProofV2 {
    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.action_identity
    }

    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.terminal_digest
    }
}

impl Debug for RuntimePendingDrainNoCandidateProofV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainNoCandidateProofV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainCompoundProofV2 {
    candidate: RuntimePendingDrainCandidateV2,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    claim_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    claimed_intent_revision: NonZeroU64,
    claimed_state_digest: RuntimePendingDrainStateDigestV2,
    acknowledgement_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    acknowledgement_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
    registry_rollover: RuntimePendingDrainRegistryRolloverProofV2,
}

impl RuntimePendingDrainCompoundProofV2 {
    pub fn candidate(&self) -> &RuntimePendingDrainCandidateV2 {
        &self.candidate
    }

    pub fn seal(&self) -> &RuntimePendingDrainRegistrySealWitnessV2 {
        &self.seal
    }

    pub fn claim_action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.claim_action_identity
    }

    pub fn claim_terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.claim_terminal_digest
    }

    pub fn claimed_intent_revision(&self) -> NonZeroU64 {
        self.claimed_intent_revision
    }

    pub fn claimed_state_digest(&self) -> &RuntimePendingDrainStateDigestV2 {
        &self.claimed_state_digest
    }

    pub fn acknowledgement_action_identity(
        &self,
    ) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.acknowledgement_action_identity
    }

    pub fn acknowledgement_terminal_digest(
        &self,
    ) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.acknowledgement_terminal_digest
    }

    pub fn acknowledged_intent_revision(&self) -> NonZeroU64 {
        self.acknowledged_intent_revision
    }

    pub fn acknowledged_state_digest(&self) -> &RuntimePendingDrainStateDigestV2 {
        &self.acknowledged_state_digest
    }

    pub fn registry_rollover(&self) -> &RuntimePendingDrainRegistryRolloverProofV2 {
        &self.registry_rollover
    }
}

impl Debug for RuntimePendingDrainCompoundProofV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainCompoundProofV2(<redacted>)")
    }
}

impl Debug for RuntimeAcceptedPendingDrainSelectionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptedPendingDrainSelectionV2(<redacted>)")
    }
}

pub struct RuntimeSelectedPendingDrainCandidateV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    acknowledgement_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainCandidateV2,
    selection_owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimeSelectedPendingDrainCandidateV2 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn candidate(&self) -> &RuntimePendingDrainCandidateV2 {
        &self.candidate
    }

    pub fn selection_owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.selection_owner_receipt
    }

    pub fn bind_registry_seal(
        self,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
    ) -> Result<RuntimeAuthorizedPendingDrainClaimV2, RuntimePendingDrainCompoundErrorV2> {
        validate_candidate_seal_binding_v2(self.authorization.request(), &self.candidate, &seal)?;
        Ok(RuntimeAuthorizedPendingDrainClaimV2 {
            authorization: self.authorization,
            acknowledgement_action_identity: self.acknowledgement_action_identity,
            candidate: self.candidate,
            seal,
            minimum_database_now: self.selection_owner_receipt.database_now,
        })
    }
}

impl Debug for RuntimeSelectedPendingDrainCandidateV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedPendingDrainCandidateV2(<redacted>)")
    }
}

pub struct RuntimeSelectedPendingDrainNoCandidateV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    selection_owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimeSelectedPendingDrainNoCandidateV2 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn selection_owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.selection_owner_receipt
    }

    pub fn complete(
        self,
        receipt: RuntimePendingDrainNoCandidateReceiptV2,
    ) -> Result<RuntimeCompletedStartupRecoveryExecutionV2, RuntimePendingDrainCompoundErrorV2>
    {
        if receipt.action_identity != *self.authorization.request().action_identity() {
            return Err(RuntimePendingDrainCompoundErrorV2::ActionMismatch);
        }
        validate_owner_receipt_v2(
            self.authorization.request(),
            &receipt.owner_receipt,
            self.selection_owner_receipt.database_now,
        )?;
        let standard_receipt = RuntimeStartupRecoveryExecutionReceiptV2 {
            correlation: self.authorization.request().correlation().clone(),
            class: RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            owner_receipt: receipt.owner_receipt,
            outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate,
        };
        Ok(self.authorization.complete_pending_drain(
            standard_receipt,
            RuntimePendingDrainExecutionProofV2::NoCandidate(
                RuntimePendingDrainNoCandidateProofV2 {
                    action_identity: receipt.action_identity,
                    terminal_digest: receipt.terminal_digest,
                },
            ),
            None,
        ))
    }
}

impl Debug for RuntimeSelectedPendingDrainNoCandidateV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedPendingDrainNoCandidateV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainNoCandidateReceiptV2 {
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimePendingDrainNoCandidateReceiptV2 {
    pub fn new(
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    ) -> Self {
        Self {
            action_identity,
            terminal_digest,
            owner_receipt,
        }
    }
}

impl Debug for RuntimePendingDrainNoCandidateReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainNoCandidateReceiptV2(<redacted>)")
    }
}

pub struct RuntimeAuthorizedPendingDrainClaimV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    acknowledgement_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainCandidateV2,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    minimum_database_now: DateTime<Utc>,
}

impl RuntimeAuthorizedPendingDrainClaimV2 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        self.authorization.request().action_identity()
    }

    pub fn candidate(&self) -> &RuntimePendingDrainCandidateV2 {
        &self.candidate
    }

    pub fn seal(&self) -> &RuntimePendingDrainRegistrySealWitnessV2 {
        &self.seal
    }

    pub fn minimum_database_now(&self) -> DateTime<Utc> {
        self.minimum_database_now
            .max(self.authorization.request().minimum_database_now())
    }

    pub fn complete(
        self,
        receipt: RuntimePendingDrainClaimReceiptV2,
    ) -> Result<RuntimeAuthorizedPendingDrainAcknowledgementV2, RuntimePendingDrainCompoundErrorV2>
    {
        if receipt.action_identity != *self.authorization.request().action_identity() {
            return Err(RuntimePendingDrainCompoundErrorV2::ActionMismatch);
        }
        if receipt.candidate != self.candidate {
            return Err(RuntimePendingDrainCompoundErrorV2::CandidateMismatch);
        }
        if receipt.seal != self.seal {
            return Err(RuntimePendingDrainCompoundErrorV2::SealMismatch);
        }
        if self.candidate.source_intent_revision.get().checked_add(1)
            != Some(receipt.claimed_intent_revision.get())
        {
            return Err(RuntimePendingDrainCompoundErrorV2::SourceContinuityMismatch);
        }
        validate_owner_receipt_v2(
            self.authorization.request(),
            &receipt.owner_receipt,
            self.minimum_database_now(),
        )?;
        Ok(RuntimeAuthorizedPendingDrainAcknowledgementV2 {
            authorization: self.authorization,
            acknowledgement_action_identity: self.acknowledgement_action_identity,
            candidate: self.candidate,
            seal: self.seal,
            claim_terminal_digest: receipt.terminal_digest,
            claimed_intent_revision: receipt.claimed_intent_revision,
            claimed_state_digest: receipt.claimed_state_digest,
            minimum_database_now: receipt.owner_receipt.database_now,
        })
    }
}

impl Debug for RuntimeAuthorizedPendingDrainClaimV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPendingDrainClaimV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainClaimReceiptV2 {
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainCandidateV2,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    claimed_intent_revision: NonZeroU64,
    claimed_state_digest: RuntimePendingDrainStateDigestV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimePendingDrainClaimReceiptV2 {
    pub fn new(
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        candidate: RuntimePendingDrainCandidateV2,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
        claimed_intent_revision: NonZeroU64,
        claimed_state_digest: RuntimePendingDrainStateDigestV2,
        terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    ) -> Self {
        Self {
            action_identity,
            candidate,
            seal,
            claimed_intent_revision,
            claimed_state_digest,
            terminal_digest,
            owner_receipt,
        }
    }
}

impl Debug for RuntimePendingDrainClaimReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainClaimReceiptV2(<redacted>)")
    }
}

pub struct RuntimeAuthorizedPendingDrainAcknowledgementV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    acknowledgement_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainCandidateV2,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    claimed_intent_revision: NonZeroU64,
    claimed_state_digest: RuntimePendingDrainStateDigestV2,
    minimum_database_now: DateTime<Utc>,
}

impl RuntimeAuthorizedPendingDrainAcknowledgementV2 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.acknowledgement_action_identity
    }

    pub fn candidate(&self) -> &RuntimePendingDrainCandidateV2 {
        &self.candidate
    }

    pub fn seal(&self) -> &RuntimePendingDrainRegistrySealWitnessV2 {
        &self.seal
    }

    pub fn claim_terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.claim_terminal_digest
    }

    pub fn claimed_intent_revision(&self) -> NonZeroU64 {
        self.claimed_intent_revision
    }

    pub fn claimed_state_digest(&self) -> &RuntimePendingDrainStateDigestV2 {
        &self.claimed_state_digest
    }

    pub fn minimum_database_now(&self) -> DateTime<Utc> {
        self.minimum_database_now
            .max(self.authorization.request().minimum_database_now())
    }

    pub fn complete(
        self,
        receipt: RuntimePendingDrainAcknowledgementReceiptV2,
    ) -> Result<RuntimeDurablyAcknowledgedPendingDrainV2, RuntimePendingDrainCompoundErrorV2> {
        if receipt.action_identity != self.acknowledgement_action_identity {
            return Err(RuntimePendingDrainCompoundErrorV2::ActionMismatch);
        }
        if receipt.claim_action_identity != *self.authorization.request().action_identity() {
            return Err(RuntimePendingDrainCompoundErrorV2::ClaimProofMismatch);
        }
        if receipt.candidate != self.candidate {
            return Err(RuntimePendingDrainCompoundErrorV2::CandidateMismatch);
        }
        if receipt.seal != self.seal {
            return Err(RuntimePendingDrainCompoundErrorV2::SealMismatch);
        }
        if receipt.source_intent_revision != self.claimed_intent_revision
            || receipt.source_state_digest != self.claimed_state_digest
            || receipt.prior_claim_terminal_digest.as_bytes()
                != self.claim_terminal_digest.as_bytes()
            || receipt.source_intent_revision.get().checked_add(1)
                != Some(receipt.acknowledged_intent_revision.get())
        {
            return Err(RuntimePendingDrainCompoundErrorV2::SourceContinuityMismatch);
        }
        validate_owner_receipt_v2(
            self.authorization.request(),
            &receipt.owner_receipt,
            self.minimum_database_now(),
        )?;
        Ok(RuntimeDurablyAcknowledgedPendingDrainV2 {
            authorization: self.authorization,
            candidate: self.candidate,
            seal: self.seal,
            claim_action_identity: receipt.claim_action_identity,
            claim_terminal_digest: self.claim_terminal_digest,
            claimed_intent_revision: self.claimed_intent_revision,
            claimed_state_digest: self.claimed_state_digest,
            acknowledgement_action_identity: self.acknowledgement_action_identity,
            acknowledgement_terminal_digest: receipt.terminal_digest,
            acknowledged_intent_revision: receipt.acknowledged_intent_revision,
            acknowledged_state_digest: receipt.acknowledged_state_digest,
            owner_receipt: receipt.owner_receipt,
        })
    }
}

impl Debug for RuntimeAuthorizedPendingDrainAcknowledgementV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPendingDrainAcknowledgementV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainAcknowledgementReceiptV2 {
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    claim_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainCandidateV2,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    source_intent_revision: NonZeroU64,
    source_state_digest: RuntimePendingDrainStateDigestV2,
    prior_claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimePendingDrainAcknowledgementReceiptV2 {
    #[expect(
        clippy::too_many_arguments,
        reason = "the acknowledgement receipt binds both durable transition stages"
    )]
    pub fn new(
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        claim_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        candidate: RuntimePendingDrainCandidateV2,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
        source_intent_revision: NonZeroU64,
        source_state_digest: RuntimePendingDrainStateDigestV2,
        prior_claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
        acknowledged_intent_revision: NonZeroU64,
        acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
        terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    ) -> Self {
        Self {
            action_identity,
            claim_action_identity,
            candidate,
            seal,
            source_intent_revision,
            source_state_digest,
            prior_claim_terminal_digest,
            acknowledged_intent_revision,
            acknowledged_state_digest,
            terminal_digest,
            owner_receipt,
        }
    }
}

impl Debug for RuntimePendingDrainAcknowledgementReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainAcknowledgementReceiptV2(<redacted>)")
    }
}

pub struct RuntimeDurablyAcknowledgedPendingDrainV2 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    candidate: RuntimePendingDrainCandidateV2,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    claim_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    claimed_intent_revision: NonZeroU64,
    claimed_state_digest: RuntimePendingDrainStateDigestV2,
    acknowledgement_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    acknowledgement_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimeDurablyAcknowledgedPendingDrainV2 {
    pub fn candidate(&self) -> &RuntimePendingDrainCandidateV2 {
        &self.candidate
    }

    pub fn seal_witness(&self) -> &RuntimePendingDrainRegistrySealWitnessV2 {
        &self.seal
    }

    pub fn complete_registry_rollover(
        self,
        unseal: RuntimePendingDrainRegistryUnsealWitnessV2,
    ) -> Result<RuntimeCompletedStartupRecoveryExecutionV2, RuntimePendingDrainCompoundErrorV2>
    {
        validate_registry_rollover_v2(&self.seal, &unseal)?;
        let RuntimePendingDrainRegistryUnsealWitnessV2 {
            process_instance_id,
            slot,
            post_slot_admission_generation,
            post_slot_observation_sequence,
            registry_observation,
        } = unseal;
        let rollover = RuntimePendingDrainRegistryRolloverProofV2 {
            process_instance_id,
            slot,
            post_slot_admission_generation,
            post_slot_observation_sequence,
            registry_observation_sequence: registry_observation.observation_sequence(),
            registry_retained_slot_count: registry_observation.retained_slot_count(),
            registry_retained_empty_tombstone_count: registry_observation
                .retained_empty_tombstone_count(),
        };
        let standard_terminal_digest = duplicate_terminal_digest_v2(&self.claim_terminal_digest);
        let standard_receipt = RuntimeStartupRecoveryExecutionReceiptV2 {
            correlation: self.authorization.request().correlation().clone(),
            class: RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            owner_receipt: self.owner_receipt,
            outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
                action_identity: self.claim_action_identity.clone(),
                terminal_digest: standard_terminal_digest,
            },
        };
        let proof = RuntimePendingDrainExecutionProofV2::Compound(Box::new(
            RuntimePendingDrainCompoundProofV2 {
                candidate: self.candidate,
                seal: self.seal,
                claim_action_identity: self.claim_action_identity,
                claim_terminal_digest: self.claim_terminal_digest,
                claimed_intent_revision: self.claimed_intent_revision,
                claimed_state_digest: self.claimed_state_digest,
                acknowledgement_action_identity: self.acknowledgement_action_identity,
                acknowledgement_terminal_digest: self.acknowledgement_terminal_digest,
                acknowledged_intent_revision: self.acknowledged_intent_revision,
                acknowledged_state_digest: self.acknowledged_state_digest,
                registry_rollover: rollover,
            },
        ));
        Ok(self.authorization.complete_pending_drain(
            standard_receipt,
            proof,
            Some(registry_observation),
        ))
    }
}

impl Debug for RuntimeDurablyAcknowledgedPendingDrainV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyAcknowledgedPendingDrainV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainRegistryUnsealWitnessV2 {
    process_instance_id: ProcessInstanceId,
    slot: RuntimeServingSlotV2,
    post_slot_admission_generation: NonZeroU64,
    post_slot_observation_sequence: NonZeroU64,
    registry_observation: RuntimeRegistryRecoveryEmptyObservationV2,
}

impl RuntimePendingDrainRegistryUnsealWitnessV2 {
    pub fn new(
        process_instance_id: ProcessInstanceId,
        slot: RuntimeServingSlotV2,
        post_slot_admission_generation: NonZeroU64,
        post_slot_observation_sequence: NonZeroU64,
        registry_observation: RuntimeRegistryRecoveryEmptyObservationV2,
    ) -> Result<Self, RuntimePendingDrainCompoundErrorV2> {
        validate_persistence_value(post_slot_admission_generation.get())?;
        validate_persistence_value(post_slot_observation_sequence.get())?;
        if registry_observation.process_instance_id() != &process_instance_id {
            return Err(RuntimePendingDrainCompoundErrorV2::RegistryRolloverMismatch);
        }
        Ok(Self {
            process_instance_id,
            slot,
            post_slot_admission_generation,
            post_slot_observation_sequence,
            registry_observation,
        })
    }
}

impl Debug for RuntimePendingDrainRegistryUnsealWitnessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainRegistryUnsealWitnessV2(<redacted>)")
    }
}

pub struct RuntimePendingDrainRegistryRolloverProofV2 {
    process_instance_id: ProcessInstanceId,
    slot: RuntimeServingSlotV2,
    post_slot_admission_generation: NonZeroU64,
    post_slot_observation_sequence: NonZeroU64,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    registry_retained_slot_count: u64,
    registry_retained_empty_tombstone_count: u64,
}

impl RuntimePendingDrainRegistryRolloverProofV2 {
    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn post_slot_admission_generation(&self) -> NonZeroU64 {
        self.post_slot_admission_generation
    }

    pub fn post_slot_observation_sequence(&self) -> NonZeroU64 {
        self.post_slot_observation_sequence
    }

    pub fn registry_observation_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }

    pub fn registry_retained_slot_count(&self) -> u64 {
        self.registry_retained_slot_count
    }

    pub fn registry_retained_empty_tombstone_count(&self) -> u64 {
        self.registry_retained_empty_tombstone_count
    }
}

impl Debug for RuntimePendingDrainRegistryRolloverProofV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainRegistryRolloverProofV2(<redacted>)")
    }
}

pub(crate) fn authorize_pending_drain_selection_v2(
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
) -> Result<RuntimeAuthorizedPendingDrainSelectionV2, RuntimePendingDrainCompoundErrorV2> {
    if authorization.request().class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
        return Err(RuntimePendingDrainCompoundErrorV2::ClassMismatch);
    }
    let acknowledgement_action_identity = authorization
        .request()
        .action_identity()
        .pending_drain_acknowledgement_successor()
        .ok_or(RuntimePendingDrainCompoundErrorV2::AuthorityRevisionOverflow)?;
    Ok(RuntimeAuthorizedPendingDrainSelectionV2 {
        authorization,
        acknowledgement_action_identity,
    })
}

fn validate_candidate_seal_binding_v2(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    candidate: &RuntimePendingDrainCandidateV2,
    seal: &RuntimePendingDrainRegistrySealWitnessV2,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    if candidate.slot != seal.slot || candidate.intent_id.canonical_bytes() != seal.seal_key {
        return Err(RuntimePendingDrainCompoundErrorV2::SealMismatch);
    }
    if request.registry_process_instance_id() != &seal.process_instance_id
        || request.registry_observation_sequence() != seal.pre_registry_observation_sequence
        || request.registry_retained_slot_count() != seal.pre_registry_retained_slot_count
        || request.registry_retained_empty_tombstone_count()
            != seal.pre_registry_retained_empty_tombstone_count
    {
        return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
    }
    Ok(())
}

fn validate_registry_rollover_v2(
    seal: &RuntimePendingDrainRegistrySealWitnessV2,
    unseal: &RuntimePendingDrainRegistryUnsealWitnessV2,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    let post = seal.post_registry_observation;
    let restored = &unseal.registry_observation;
    if unseal.process_instance_id != seal.process_instance_id
        || unseal.slot != seal.slot
        || seal.post_slot_admission_generation.get().checked_add(1)
            != Some(unseal.post_slot_admission_generation.get())
        || seal.post_slot_observation_sequence.get().checked_add(1)
            != Some(unseal.post_slot_observation_sequence.get())
        || post.observation_sequence.get().checked_add(1)
            != Some(restored.observation_sequence().get())
        || post.retained_slot_count != restored.retained_slot_count()
        || restored.retained_slot_count() != restored.retained_empty_tombstone_count()
    {
        return Err(RuntimePendingDrainCompoundErrorV2::RegistryRolloverMismatch);
    }
    Ok(())
}

fn duplicate_terminal_digest_v2(
    digest: &RuntimeStartupRecoveryExecutionTerminalDigestV2,
) -> RuntimeStartupRecoveryExecutionTerminalDigestV2 {
    RuntimeStartupRecoveryExecutionTerminalDigestV2::new(*digest.as_bytes())
        .expect("accepted runtime pending drain terminal digest is nonzero")
}

fn validate_registry_seal_transition_v2(
    input: &RuntimePendingDrainRegistrySealWitnessInputV2,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    if input.pre_registry_retained_slot_count != input.pre_registry_retained_empty_tombstone_count {
        return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
    }
    let post = input.post_registry_observation;
    validate_persistence_value(post.observation_sequence.get())?;
    for value in [
        post.retained_slot_count,
        post.retained_empty_tombstone_count,
        post.staged_route_count,
        post.serving_route_count,
        post.draining_route_count,
        post.sealed_slot_count,
        post.active_interaction_count,
        post.failed_closed_slot_count,
    ] {
        validate_persistence_count(value)?;
    }
    if post.registry_failed_closed
        || post.staged_route_count != 0
        || post.serving_route_count != 0
        || post.draining_route_count != 0
        || post.sealed_slot_count != 1
        || post.active_interaction_count != 0
        || post.failed_closed_slot_count != 0
        || input.pre_registry_observation_sequence.get().checked_add(1)
            != Some(post.observation_sequence.get())
    {
        return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
    }
    match input.pre_slot_observation {
        None => {
            if input.post_slot_admission_generation != NonZeroU64::MIN
                || input.post_slot_observation_sequence != NonZeroU64::MIN
                || input.pre_registry_retained_slot_count.checked_add(1)
                    != Some(post.retained_slot_count)
                || input.pre_registry_retained_empty_tombstone_count
                    != post.retained_empty_tombstone_count
            {
                return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
            }
        }
        Some(source) => {
            validate_persistence_value(source.admission_generation.get())?;
            validate_persistence_value(source.observation_sequence.get())?;
            if source.admission_generation.get().checked_add(1)
                != Some(input.post_slot_admission_generation.get())
                || source.observation_sequence.get().checked_add(1)
                    != Some(input.post_slot_observation_sequence.get())
                || input.pre_registry_retained_slot_count != post.retained_slot_count
                || input
                    .pre_registry_retained_empty_tombstone_count
                    .checked_sub(1)
                    != Some(post.retained_empty_tombstone_count)
            {
                return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
            }
        }
    }
    if post.retained_empty_tombstone_count.checked_add(1) != Some(post.retained_slot_count) {
        return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
    }
    Ok(())
}

fn validate_owner_receipt_v2(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    observed: &RuntimeGatewayOwnerLeaseReceiptV1,
    minimum_database_now: DateTime<Utc>,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    if observed.lease_id != *request.gateway_owner_lease_id()
        || observed.owner_revision != request.expected_owner_revision()
        || observed.expires_at != request.expected_owner_expires_at()
    {
        return Err(RuntimePendingDrainCompoundErrorV2::OwnerMismatch);
    }
    if observed.database_now < minimum_database_now {
        return Err(RuntimePendingDrainCompoundErrorV2::DatabaseClockRegressed);
    }
    if observed.database_lease_duration().is_none() {
        return Err(RuntimePendingDrainCompoundErrorV2::OwnerNotCurrent);
    }
    Ok(())
}

fn validate_persistence_value(value: u64) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    if value == 0 || value > i64::MAX as u64 {
        Err(RuntimePendingDrainCompoundErrorV2::RegistryValueOutOfRange)
    } else {
        Ok(())
    }
}

fn validate_persistence_count(value: u64) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    if value > i64::MAX as u64 {
        Err(RuntimePendingDrainCompoundErrorV2::RegistryValueOutOfRange)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePendingDrainCompoundErrorV2 {
    #[error("runtime pending drain recovery class does not match")]
    ClassMismatch,
    #[error("runtime pending drain recovery correlation does not match")]
    CorrelationMismatch,
    #[error("runtime pending drain recovery owner does not match")]
    OwnerMismatch,
    #[error("runtime pending drain recovery database clock regressed")]
    DatabaseClockRegressed,
    #[error("runtime pending drain recovery owner is not current")]
    OwnerNotCurrent,
    #[error("runtime pending drain candidate target does not match its slot")]
    CandidateTargetMismatch,
    #[error("runtime pending drain intent revision overflows")]
    IntentRevisionOverflow,
    #[error("runtime pending drain authority revision overflows")]
    AuthorityRevisionOverflow,
    #[error("runtime pending drain digest is zero")]
    ZeroDigest,
    #[error("runtime pending drain registry value is outside the persistence domain")]
    RegistryValueOutOfRange,
    #[error("runtime pending drain registry evidence does not match")]
    RegistryMismatch,
    #[error("runtime pending drain seal evidence does not match")]
    SealMismatch,
    #[error("runtime pending drain action identity does not match")]
    ActionMismatch,
    #[error("runtime pending drain candidate does not match")]
    CandidateMismatch,
    #[error("runtime pending drain source and successor do not match")]
    SourceContinuityMismatch,
    #[error("runtime pending drain claim proof does not match")]
    ClaimProofMismatch,
    #[error("runtime pending drain registry rollover does not match")]
    RegistryRolloverMismatch,
}
