use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimV2, RuntimeDrainIntentIdV2,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    RuntimeServingSlotV2,
};
use automation_runtime_convergence::RuntimeDeploymentTargetV1;
use chrono::{DateTime, Utc};

use super::{
    duplicate_terminal_digest_v2, validate_candidate_seal_binding_v2, validate_owner_receipt_v2,
    validate_registry_rollover_v2, RuntimePendingDrainCandidateV2,
    RuntimePendingDrainCompoundErrorV2, RuntimePendingDrainExecutionProofV2,
    RuntimePendingDrainRegistryRolloverProofV2, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainRegistryUnsealWitnessV2, RuntimePendingDrainStateDigestV2,
    RuntimeSelectedPendingDrainCandidateV2, RuntimeSelectedPendingDrainNoCandidateV2,
};
use crate::{
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeCompletedStartupRecoveryExecutionV2,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryExecutionActionIdentityV2,
    RuntimeStartupRecoveryExecutionCorrelationV2, RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
    RuntimeStartupRecoveryExecutionReceiptV2, RuntimeStartupRecoveryExecutionTerminalDigestV2,
};

#[cfg(test)]
mod tests;

pub struct RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3 {
    pub source: RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    pub source_state_digest: RuntimePendingDrainStateDigestV2,
    pub predecessor_claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub product_mutation_request_sha256: [u8; 32],
    pub drain_intent_request_sha256: [u8; 32],
}

impl Debug for RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3(<redacted>)")
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
    intent_id: RuntimeDrainIntentIdV2,
    slot: RuntimeServingSlotV2,
    expected_target: RuntimeDeploymentTargetV1,
    source_intent_revision: NonZeroU64,
    source_state_digest: RuntimePendingDrainStateDigestV2,
    predecessor_claim: RuntimeDrainClaimV2,
    predecessor_claim_terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    product_mutation_request_sha256: [u8; 32],
    drain_intent_request_sha256: [u8; 32],
}

impl Clone for RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
    fn clone(&self) -> Self {
        Self {
            intent_id: self.intent_id.clone(),
            slot: self.slot.clone(),
            expected_target: self.expected_target.clone(),
            source_intent_revision: self.source_intent_revision,
            source_state_digest: self.source_state_digest.clone(),
            predecessor_claim: self.predecessor_claim.clone(),
            predecessor_claim_terminal_digest:
                RuntimeStartupRecoveryExecutionTerminalDigestV2::new(
                    *self.predecessor_claim_terminal_digest.as_bytes(),
                )
                .expect("checked predecessor claim terminal digest is nonzero"),
            product_mutation_request_sha256: self.product_mutation_request_sha256,
            drain_intent_request_sha256: self.drain_intent_request_sha256,
        }
    }
}

impl RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
    pub fn new(
        input: RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3,
    ) -> Result<Self, RuntimePendingDrainCompoundErrorV2> {
        let intent = input.source.canonical().intent();
        let key = intent.key();
        let predecessor_claim = intent
            .state()
            .pending_claim()
            .cloned()
            .ok_or(RuntimePendingDrainCompoundErrorV2::InvalidPreviousClaimCandidate)?;
        if !key.slot.matches_target(&key.expected_target) {
            return Err(RuntimePendingDrainCompoundErrorV2::CandidateTargetMismatch);
        }
        let seal = predecessor_claim.progress().seal();
        if predecessor_claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed
            || seal.expected_route().is_some()
            || seal.intent_id() != &key.intent_id
            || seal.slot() != &key.slot
        {
            return Err(RuntimePendingDrainCompoundErrorV2::InvalidPreviousClaimCandidate);
        }
        validate_successor_value_v3(
            intent.intent_revision().get(),
            RuntimePendingDrainCompoundErrorV2::IntentRevisionOverflow,
        )?;
        validate_successor_value_v3(
            predecessor_claim.claim_revision().get(),
            RuntimePendingDrainCompoundErrorV2::ClaimRevisionOverflow,
        )?;
        validate_successor_value_v3(
            predecessor_claim.controller_fencing_token().get(),
            RuntimePendingDrainCompoundErrorV2::ControllerFenceOverflow,
        )?;
        Ok(Self {
            intent_id: key.intent_id.clone(),
            slot: key.slot.clone(),
            expected_target: key.expected_target.clone(),
            source_intent_revision: intent.intent_revision(),
            source_state_digest: input.source_state_digest,
            predecessor_claim,
            predecessor_claim_terminal_digest: input.predecessor_claim_terminal_digest,
            product_mutation_request_sha256: input.product_mutation_request_sha256,
            drain_intent_request_sha256: input.drain_intent_request_sha256,
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

    pub fn source_state_digest(&self) -> &RuntimePendingDrainStateDigestV2 {
        &self.source_state_digest
    }

    pub fn predecessor_claim_terminal_digest(
        &self,
    ) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.predecessor_claim_terminal_digest
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn predecessor_claim(&self) -> &RuntimeDrainClaimV2 {
        &self.predecessor_claim
    }

    pub fn product_mutation_request_sha256(&self) -> &[u8; 32] {
        &self.product_mutation_request_sha256
    }

    pub fn drain_intent_request_sha256(&self) -> &[u8; 32] {
        &self.drain_intent_request_sha256
    }

    pub fn claim_expires_at(&self) -> DateTime<Utc> {
        self.predecessor_claim.expires_at()
    }

    fn seal_binding_candidate(&self) -> RuntimePendingDrainCandidateV2 {
        RuntimePendingDrainCandidateV2 {
            intent_id: self.intent_id.clone(),
            slot: self.slot.clone(),
            expected_target: self.expected_target.clone(),
            source_intent_revision: self.source_intent_revision,
            source_state_digest: self.source_state_digest.clone(),
        }
    }
}

impl Debug for RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainPreviousOwnerClaimedCandidateV3(<redacted>)")
    }
}

pub struct RuntimePendingDrainSelectionReceiptV3 {
    correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    outcome: RuntimePendingDrainSelectionOutcomeV3,
}

impl RuntimePendingDrainSelectionReceiptV3 {
    pub fn new(
        correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        outcome: RuntimePendingDrainSelectionOutcomeV3,
    ) -> Self {
        Self {
            correlation,
            owner_receipt,
            outcome,
        }
    }
}

impl Debug for RuntimePendingDrainSelectionReceiptV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainSelectionReceiptV3(<redacted>)")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimePendingDrainSelectionOutcomeV3 {
    NoCandidate,
    Unclaimed(RuntimePendingDrainCandidateV2),
    FreshPreviousOwner(RuntimePendingDrainPreviousOwnerClaimedCandidateV3),
    ExpiredPreviousOwner(RuntimePendingDrainPreviousOwnerClaimedCandidateV3),
}

pub trait RuntimePendingDrainSelectionPortV3 {
    type Error;

    fn select_pending_drain_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainSelectionReceiptV3, Self::Error>> + Send;
}

pub trait RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3 {
    type Error;

    fn execute_pending_drain_succession_acknowledgement(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<RuntimePendingDrainSuccessionAcknowledgementReceiptV3, Self::Error>,
    > + Send;
}

pub struct RuntimeAuthorizedPendingDrainSelectionV3 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
}

impl RuntimeAuthorizedPendingDrainSelectionV3 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        self.authorization.request().action_identity()
    }

    pub fn accept_selection(
        self,
        receipt: RuntimePendingDrainSelectionReceiptV3,
    ) -> Result<RuntimeAcceptedPendingDrainSelectionV3, RuntimePendingDrainCompoundErrorV2> {
        if receipt.correlation != *self.authorization.request().correlation() {
            return Err(RuntimePendingDrainCompoundErrorV2::CorrelationMismatch);
        }
        validate_owner_receipt_v2(
            self.authorization.request(),
            &receipt.owner_receipt,
            self.authorization.request().minimum_database_now(),
        )?;
        match receipt.outcome {
            RuntimePendingDrainSelectionOutcomeV3::NoCandidate => {
                Ok(RuntimeAcceptedPendingDrainSelectionV3::NoCandidate(
                    Box::new(RuntimeSelectedPendingDrainNoCandidateV2 {
                        authorization: self.authorization,
                        selection_owner_receipt: receipt.owner_receipt,
                    }),
                ))
            }
            RuntimePendingDrainSelectionOutcomeV3::Unclaimed(candidate) => {
                let acknowledgement_action_identity = self
                    .authorization
                    .request()
                    .action_identity()
                    .pending_drain_acknowledgement_successor()
                    .ok_or(RuntimePendingDrainCompoundErrorV2::AuthorityRevisionOverflow)?;
                Ok(RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(Box::new(
                    RuntimeSelectedPendingDrainCandidateV2 {
                        authorization: self.authorization,
                        acknowledgement_action_identity,
                        candidate,
                        selection_owner_receipt: receipt.owner_receipt,
                    },
                )))
            }
            RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate) => {
                validate_previous_owner_candidate_v3(
                    self.authorization.request(),
                    &receipt.owner_receipt,
                    &candidate,
                )?;
                let retry_after = fresh_retry_v3(&receipt.owner_receipt, &candidate)?;
                Ok(RuntimeAcceptedPendingDrainSelectionV3::FreshPreviousOwner(
                    Box::new(RuntimePendingDrainFreshPreviousOwnerSelectionV3 {
                        authorization: self.authorization,
                        candidate,
                        retry_after,
                        selection_database_now: receipt.owner_receipt.database_now,
                        owner_receipt: receipt.owner_receipt,
                    }),
                ))
            }
            RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(candidate) => {
                validate_previous_owner_candidate_v3(
                    self.authorization.request(),
                    &receipt.owner_receipt,
                    &candidate,
                )?;
                if receipt.owner_receipt.database_now < candidate.claim_expires_at() {
                    return Err(
                        RuntimePendingDrainCompoundErrorV2::PreviousClaimClassificationMismatch,
                    );
                }
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(Box::new(
                        RuntimeSelectedPendingDrainSuccessionV3 {
                            authorization: self.authorization,
                            candidate,
                            selection_owner_receipt: receipt.owner_receipt,
                        },
                    )),
                )
            }
        }
    }
}

impl Debug for RuntimeAuthorizedPendingDrainSelectionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPendingDrainSelectionV3(<redacted>)")
    }
}

pub enum RuntimeAcceptedPendingDrainSelectionV3 {
    NoCandidate(Box<RuntimeSelectedPendingDrainNoCandidateV2>),
    Unclaimed(Box<RuntimeSelectedPendingDrainCandidateV2>),
    FreshPreviousOwner(Box<RuntimePendingDrainFreshPreviousOwnerSelectionV3>),
    ExpiredPreviousOwner(Box<RuntimeSelectedPendingDrainSuccessionV3>),
}

impl Debug for RuntimeAcceptedPendingDrainSelectionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptedPendingDrainSelectionV3(<redacted>)")
    }
}

pub struct RuntimePendingDrainFreshPreviousOwnerSelectionV3 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    retry_after: Duration,
    selection_database_now: DateTime<Utc>,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimePendingDrainFreshPreviousOwnerSelectionV3 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn candidate(&self) -> &RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
        &self.candidate
    }

    pub fn retry_after(&self) -> Duration {
        self.retry_after
    }

    pub fn complete(self) -> RuntimeCompletedStartupRecoveryExecutionV2 {
        let action_identity = self.authorization.request().action_identity().clone();
        let standard_receipt = RuntimeStartupRecoveryExecutionReceiptV2 {
            correlation: self.authorization.request().correlation().clone(),
            class: RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            owner_receipt: self.owner_receipt,
            outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter {
                retry_after: self.retry_after,
            },
        };
        self.authorization.complete_pending_drain(
            standard_receipt,
            RuntimePendingDrainExecutionProofV2::Deferred(Box::new(
                RuntimePendingDrainDeferredSelectionProofV3 {
                    action_identity,
                    candidate: self.candidate,
                    selection_database_now: self.selection_database_now,
                    retry_after: self.retry_after,
                },
            )),
            None,
        )
    }
}

impl Debug for RuntimePendingDrainFreshPreviousOwnerSelectionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFreshPreviousOwnerSelectionV3(<redacted>)")
    }
}

pub struct RuntimePendingDrainDeferredSelectionProofV3 {
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    selection_database_now: DateTime<Utc>,
    retry_after: Duration,
}

impl RuntimePendingDrainDeferredSelectionProofV3 {
    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.action_identity
    }

    pub fn candidate(&self) -> &RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
        &self.candidate
    }

    pub fn selection_database_now(&self) -> DateTime<Utc> {
        self.selection_database_now
    }

    pub fn claim_expires_at(&self) -> DateTime<Utc> {
        self.candidate.claim_expires_at()
    }

    pub fn retry_after(&self) -> Duration {
        self.retry_after
    }
}

impl Debug for RuntimePendingDrainDeferredSelectionProofV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainDeferredSelectionProofV3(<redacted>)")
    }
}

pub struct RuntimeSelectedPendingDrainSuccessionV3 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    selection_owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimeSelectedPendingDrainSuccessionV3 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn candidate(&self) -> &RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
        &self.candidate
    }

    pub fn selection_owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.selection_owner_receipt
    }

    pub fn bind_registry_seal(
        self,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
    ) -> Result<
        RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
        RuntimePendingDrainCompoundErrorV2,
    > {
        let seal_candidate = self.candidate.seal_binding_candidate();
        validate_candidate_seal_binding_v2(self.authorization.request(), &seal_candidate, &seal)?;
        Ok(RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3 {
            authorization: self.authorization,
            candidate: self.candidate,
            seal,
            minimum_database_now: self.selection_owner_receipt.database_now,
        })
    }
}

impl Debug for RuntimeSelectedPendingDrainSuccessionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedPendingDrainSuccessionV3(<redacted>)")
    }
}

pub struct RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    minimum_database_now: DateTime<Utc>,
}

impl RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        self.authorization.request().action_identity()
    }

    pub fn candidate(&self) -> &RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
        &self.candidate
    }

    pub fn seal(&self) -> &RuntimePendingDrainRegistrySealWitnessV2 {
        &self.seal
    }

    pub fn minimum_database_now(&self) -> DateTime<Utc> {
        self.minimum_database_now
            .max(self.authorization.request().minimum_database_now())
            .max(self.candidate.claim_expires_at())
    }

    pub fn complete(
        self,
        receipt: RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
    ) -> Result<
        RuntimeDurablyAcknowledgedPendingDrainSuccessionV3,
        RuntimePendingDrainCompoundErrorV2,
    > {
        if receipt.action_identity != *self.authorization.request().action_identity() {
            return Err(RuntimePendingDrainCompoundErrorV2::ActionMismatch);
        }
        if receipt.candidate != self.candidate {
            return Err(RuntimePendingDrainCompoundErrorV2::CandidateMismatch);
        }
        if receipt.seal != self.seal {
            return Err(RuntimePendingDrainCompoundErrorV2::SealMismatch);
        }
        if self.candidate.source_intent_revision().get().checked_add(1)
            != Some(receipt.acknowledged_intent_revision.get())
        {
            return Err(RuntimePendingDrainCompoundErrorV2::SourceContinuityMismatch);
        }
        validate_owner_receipt_v2(
            self.authorization.request(),
            &receipt.owner_receipt,
            self.minimum_database_now(),
        )?;
        Ok(RuntimeDurablyAcknowledgedPendingDrainSuccessionV3 {
            authorization: self.authorization,
            candidate: self.candidate,
            seal: self.seal,
            terminal_digest: receipt.terminal_digest,
            acknowledged_intent_revision: receipt.acknowledged_intent_revision,
            acknowledged_state_digest: receipt.acknowledged_state_digest,
            owner_receipt: receipt.owner_receipt,
        })
    }
}

impl Debug for RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3(<redacted>)")
    }
}

pub struct RuntimePendingDrainSuccessionAcknowledgementReceiptV3 {
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimePendingDrainSuccessionAcknowledgementReceiptV3 {
    pub fn new(
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
        acknowledged_intent_revision: NonZeroU64,
        acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
        terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    ) -> Self {
        Self {
            action_identity,
            candidate,
            seal,
            acknowledged_intent_revision,
            acknowledged_state_digest,
            terminal_digest,
            owner_receipt,
        }
    }
}

impl Debug for RuntimePendingDrainSuccessionAcknowledgementReceiptV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainSuccessionAcknowledgementReceiptV3(<redacted>)")
    }
}

pub struct RuntimeDurablyAcknowledgedPendingDrainSuccessionV3 {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimeDurablyAcknowledgedPendingDrainSuccessionV3 {
    pub fn candidate(&self) -> &RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
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
        let action_identity = self.authorization.request().action_identity().clone();
        let standard_terminal_digest = duplicate_terminal_digest_v2(&self.terminal_digest);
        let standard_receipt = RuntimeStartupRecoveryExecutionReceiptV2 {
            correlation: self.authorization.request().correlation().clone(),
            class: RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            owner_receipt: self.owner_receipt,
            outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
                action_identity: action_identity.clone(),
                terminal_digest: standard_terminal_digest,
            },
        };
        let proof = RuntimePendingDrainExecutionProofV2::Succession(Box::new(
            RuntimePendingDrainSuccessionProofV3 {
                candidate: self.candidate,
                seal: self.seal,
                action_identity,
                terminal_digest: self.terminal_digest,
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

impl Debug for RuntimeDurablyAcknowledgedPendingDrainSuccessionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyAcknowledgedPendingDrainSuccessionV3(<redacted>)")
    }
}

pub struct RuntimePendingDrainSuccessionProofV3 {
    candidate: RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    seal: RuntimePendingDrainRegistrySealWitnessV2,
    action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: RuntimePendingDrainStateDigestV2,
    registry_rollover: RuntimePendingDrainRegistryRolloverProofV2,
}

impl RuntimePendingDrainSuccessionProofV3 {
    pub fn candidate(&self) -> &RuntimePendingDrainPreviousOwnerClaimedCandidateV3 {
        &self.candidate
    }

    pub fn seal(&self) -> &RuntimePendingDrainRegistrySealWitnessV2 {
        &self.seal
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.action_identity
    }

    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.terminal_digest
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

impl Debug for RuntimePendingDrainSuccessionProofV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainSuccessionProofV3(<redacted>)")
    }
}

pub(crate) fn authorize_pending_drain_selection_v3(
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
) -> Result<RuntimeAuthorizedPendingDrainSelectionV3, RuntimePendingDrainCompoundErrorV2> {
    if authorization.request().class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
        return Err(RuntimePendingDrainCompoundErrorV2::ClassMismatch);
    }
    Ok(RuntimeAuthorizedPendingDrainSelectionV3 { authorization })
}

fn validate_successor_value_v3(
    value: u64,
    error: RuntimePendingDrainCompoundErrorV2,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    if value
        .checked_add(1)
        .is_none_or(|next| next > i64::MAX as u64)
    {
        Err(error)
    } else {
        Ok(())
    }
}

fn validate_previous_owner_candidate_v3(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    current_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    let predecessor_owner = candidate.predecessor_claim.gateway_owner_lease_id();
    if predecessor_owner.process_instance_id == current_owner.lease_id.process_instance_id {
        return Err(RuntimePendingDrainCompoundErrorV2::PreviousOwnerProcessNotDistinct);
    }
    if predecessor_owner.gateway_shard_id != current_owner.lease_id.gateway_shard_id {
        return Err(RuntimePendingDrainCompoundErrorV2::PreviousOwnerShardMismatch);
    }
    if predecessor_owner.lease_epoch >= current_owner.lease_id.lease_epoch {
        return Err(RuntimePendingDrainCompoundErrorV2::PreviousOwnerEpochNotOlder);
    }
    if request.registry_process_instance_id() != &current_owner.lease_id.process_instance_id {
        return Err(RuntimePendingDrainCompoundErrorV2::RegistryMismatch);
    }
    Ok(())
}

fn fresh_retry_v3(
    current_owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> Result<Duration, RuntimePendingDrainCompoundErrorV2> {
    let remaining_predecessor = candidate
        .claim_expires_at()
        .signed_duration_since(current_owner.database_now)
        .to_std()
        .ok()
        .filter(|remaining| !remaining.is_zero())
        .ok_or(RuntimePendingDrainCompoundErrorV2::PreviousClaimClassificationMismatch)?;
    let remaining_owner = current_owner
        .database_lease_duration()
        .ok_or(RuntimePendingDrainCompoundErrorV2::OwnerNotCurrent)?;
    Ok(Duration::from_secs(1)
        .min(remaining_predecessor)
        .min(remaining_owner))
}
