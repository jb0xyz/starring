mod wire;

#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::{ControllerId, FencingToken};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::v2_drain_intent_receipt::validate_succession_acknowledgement_v2;
use crate::{
    RuntimeCanonicalRouteMutationProvenanceV2, RuntimeCanonicalValueErrorV2,
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeDrainAcknowledgementSourceV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimErrorV2,
    RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2,
    RuntimeDrainClaimV2, RuntimeDrainIntentDigestV2, RuntimeDrainIntentKeyV2,
    RuntimeDrainIntentReceiptErrorV2, RuntimeDrainIntentReceiptV2, RuntimeDrainIntentStateErrorV2,
    RuntimeDrainIntentStateKindV2, RuntimeDrainIntentV2,
    RuntimeDrainSuccessionAcknowledgementExpectationV2,
    RuntimeDrainSuccessionAcknowledgementSourceV2, RuntimePersistedProductDrainRootV2,
    RuntimeRouteAbsentAcknowledgementV2, RuntimeRouteMutationProvenanceV2,
};

const DRAIN_INTENT_STATE_MAX_OCTETS: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentCanonicalStateKindV2 {
    PendingUnclaimed,
    PendingClaimed,
    PendingRefenced,
    RouteAbsentAcknowledged,
    Consumed,
    Cancelled,
}

impl RuntimeDrainIntentCanonicalStateKindV2 {
    pub fn persisted_state(self) -> &'static str {
        match self {
            Self::PendingUnclaimed | Self::PendingClaimed | Self::PendingRefenced => "pending",
            Self::RouteAbsentAcknowledged => "route_absent_acknowledged",
            Self::Consumed => "consumed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentCanonicalStateFieldV2 {
    FormatVersion,
    IntentRevision,
    ResultingRevision,
    ConsumedAt,
    CancelledAt,
    DrainIntentId,
    ProductOperationId,
    ProductMutationDigest,
    DrainIntentDigest,
    TenantId,
    InstallationId,
    DeploymentId,
    ExpectedRevision,
    SlotGuildId,
    SlotRuleSetKey,
    TargetGuildId,
    TargetRuleSetKey,
    TargetVersion,
    TargetContentHash,
    TargetBindingRevision,
    TargetBindingFingerprint,
    MutationKind,
    GatewayShardId,
    GatewayProcessInstanceId,
    GatewayLeaseEpoch,
    GatewayBuildRevision,
    OwnerRevision,
    ProcessInstanceId,
    ControllerId,
    ControllerFencingToken,
    ClaimEpoch,
    ClaimRevision,
    ClaimExpiresAt,
    SealGeneration,
    SealObservationSequence,
    RouteRuntimeGeneration,
    RouteProcessInstanceId,
    RouteControllerFencingToken,
    RouteIncarnation,
    RefenceObservationSequence,
    RefencedAt,
    Provenance,
    AcknowledgementObservationSequence,
    AcknowledgedAt,
    CertificationOperationId,
    CertificationIntentFingerprint,
    CertificationAttestationDigest,
    ServingLeaseEpoch,
    ServingRevision,
    DisconnectedRevision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentCanonicalStateCorrelationV2 {
    ImmutableRoot,
    PersistedState,
    PendingProgress,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDrainIntentCanonicalStateErrorV2 {
    #[error("runtime drain-intent state payload exceeds its size limit")]
    PayloadTooLarge,
    #[error("runtime drain-intent state payload encoding failed")]
    Encoding,
    #[error("runtime drain-intent state payload decoding failed")]
    Decoding,
    #[error("runtime drain-intent state payload format version is unsupported")]
    UnsupportedFormatVersion,
    #[error("runtime drain-intent state payload has a noncanonical representation")]
    NonCanonicalEncoding,
    #[error("runtime drain-intent state field {field:?} is invalid")]
    InvalidField {
        field: RuntimeDrainIntentCanonicalStateFieldV2,
    },
    #[error("runtime drain-intent state field {field:?} is invalid: {reason}")]
    CanonicalValue {
        field: RuntimeDrainIntentCanonicalStateFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error("runtime drain-intent state fields disagree on {field:?}")]
    CorrelationMismatch {
        field: RuntimeDrainIntentCanonicalStateCorrelationV2,
    },
    #[error(transparent)]
    State(#[from] RuntimeDrainIntentStateErrorV2),
    #[error(transparent)]
    Claim(#[from] RuntimeDrainClaimErrorV2),
    #[error(transparent)]
    Receipt(#[from] RuntimeDrainIntentReceiptErrorV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCanonicalDrainIntentStateV2 {
    intent: RuntimeDrainIntentV2,
    state_bytes: Box<[u8]>,
}

impl RuntimeCanonicalDrainIntentStateV2 {
    pub fn from_intent(
        intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let state_bytes = wire::encode_state(&intent)?.into_boxed_slice();
        Ok(Self {
            intent,
            state_bytes,
        })
    }

    pub fn from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        persisted_state: &str,
        state_bytes: &[u8],
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let intent = wire::decode_state(root, intent_revision, state_bytes)?;
        let canonical_state = canonical_state_kind(&intent)?;
        if canonical_state.persisted_state() != persisted_state {
            return Err(
                RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                    field: RuntimeDrainIntentCanonicalStateCorrelationV2::PersistedState,
                },
            );
        }
        Ok(Self {
            intent,
            state_bytes: state_bytes.to_vec().into_boxed_slice(),
        })
    }

    pub fn intent(&self) -> &RuntimeDrainIntentV2 {
        &self.intent
    }

    pub fn state_kind(
        &self,
    ) -> Result<RuntimeDrainIntentCanonicalStateKindV2, RuntimeDrainIntentCanonicalStateErrorV2>
    {
        canonical_state_kind(&self.intent)
    }

    pub fn persisted_state(&self) -> Result<&'static str, RuntimeDrainIntentCanonicalStateErrorV2> {
        Ok(self.state_kind()?.persisted_state())
    }

    pub fn state_bytes(&self) -> &[u8] {
        &self.state_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2 {
    pub recovery_witness: RuntimeClosedRecoveryRouteWitnessV2,
    pub controller_id: ControllerId,
    pub controller_fencing_token: FencingToken,
    pub claim_epoch: NonZeroU64,
    pub claim_revision: NonZeroU64,
    pub claim_expires_at: DateTime<Utc>,
    pub seal_generation: NonZeroU64,
    pub seal_observation_sequence: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2 {
    pub acknowledgement_observation_sequence: NonZeroU64,
    pub certification: RuntimeDrainCertificationResolutionV2,
    pub acknowledged_at: DateTime<Utc>,
    pub recovery_witness: RuntimeClosedRecoveryRouteWitnessV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2 {
    pub database_now: DateTime<Utc>,
    pub recovery_witness: RuntimeClosedRecoveryRouteWitnessV2,
    pub controller_id: ControllerId,
    pub seal_generation: NonZeroU64,
    pub seal_observation_sequence: NonZeroU64,
    pub acknowledgement_observation_sequence: NonZeroU64,
    pub certification: RuntimeDrainCertificationResolutionV2,
    pub acknowledged_at: DateTime<Utc>,
}

pub struct RuntimeCompactPendingDrainSuccessionValidationInputV2<'a> {
    pub source_intent_revision: NonZeroU64,
    pub source_state_digest: [u8; 32],
    pub predecessor_claim_source_digest: [u8; 32],
    pub predecessor_claim: &'a RuntimeDrainClaimV2,
    pub succession: &'a RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
    pub successor_state_bytes: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeValidatedCompactPendingDrainSuccessionV2 {
    key: RuntimeDrainIntentKeyV2,
    drain_intent_digest: RuntimeDrainIntentDigestV2,
    successor_intent_revision: NonZeroU64,
    successor_state_digest: [u8; 32],
    certification: RuntimeDrainCertificationResolutionV2,
}

impl RuntimeValidatedCompactPendingDrainSuccessionV2 {
    pub fn key(&self) -> &RuntimeDrainIntentKeyV2 {
        &self.key
    }

    pub fn drain_intent_digest(&self) -> &RuntimeDrainIntentDigestV2 {
        &self.drain_intent_digest
    }

    pub fn successor_intent_revision(&self) -> NonZeroU64 {
        self.successor_intent_revision
    }

    pub fn successor_state_digest(&self) -> &[u8; 32] {
        &self.successor_state_digest
    }

    pub fn certification(&self) -> &RuntimeDrainCertificationResolutionV2 {
        &self.certification
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCompactPendingDrainSuccessionValidationErrorV2 {
    #[error(
        "runtime compact pending-drain succession predecessor claim source revision is absent"
    )]
    PredecessorClaimSourceRevisionMissing,
    #[error(
        "runtime compact pending-drain succession predecessor claim source digest does not match"
    )]
    PredecessorClaimSourceDigestMismatch,
    #[error("runtime compact pending-drain succession source state digest does not match")]
    SourceStateDigestMismatch,
    #[error("runtime compact pending-drain succession intent revision is not the exact successor")]
    SuccessorIntentRevisionMismatch,
    #[error(transparent)]
    CanonicalState(#[from] RuntimeDrainIntentCanonicalStateErrorV2),
    #[error(transparent)]
    Succession(#[from] RuntimeDrainIntentReceiptErrorV2),
}

pub fn validate_compact_pending_drain_succession_v2(
    input: RuntimeCompactPendingDrainSuccessionValidationInputV2<'_>,
) -> Result<
    RuntimeValidatedCompactPendingDrainSuccessionV2,
    RuntimeCompactPendingDrainSuccessionValidationErrorV2,
> {
    let predecessor_claim_source_revision = input
        .source_intent_revision
        .get()
        .checked_sub(1)
        .and_then(NonZeroU64::new)
        .ok_or(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::PredecessorClaimSourceRevisionMissing,
        )?;
    let successor = wire::decode_compact_succession_successor_v2(input.successor_state_bytes)?;
    let source_state_bytes = wire::encode_compact_pending_claimed_source_v2(
        &successor.key,
        &successor.drain_intent_digest,
        input.source_intent_revision,
        input.predecessor_claim,
    )?;
    let source_state_digest: [u8; 32] = Sha256::digest(&source_state_bytes).into();
    if source_state_digest != input.source_state_digest {
        return Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::SourceStateDigestMismatch,
        );
    }
    let predecessor_claim_source_bytes = wire::encode_compact_pending_unclaimed_source_v2(
        &successor.key,
        &successor.drain_intent_digest,
        predecessor_claim_source_revision,
    )?;
    let predecessor_claim_source_digest: [u8; 32] =
        Sha256::digest(&predecessor_claim_source_bytes).into();
    if predecessor_claim_source_digest != input.predecessor_claim_source_digest {
        return Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::PredecessorClaimSourceDigestMismatch,
        );
    }
    if input.source_intent_revision.get().checked_add(1) != Some(successor.intent_revision.get()) {
        return Err(
            RuntimeCompactPendingDrainSuccessionValidationErrorV2::SuccessorIntentRevisionMismatch,
        );
    }
    let succession = input.succession;
    validate_succession_acknowledgement_v2(
        input.predecessor_claim,
        &successor.acknowledgement,
        &RuntimeDrainSuccessionAcknowledgementExpectationV2 {
            database_now: succession.database_now,
            recovery_witness: succession.recovery_witness.clone(),
            controller_id: succession.controller_id.clone(),
            seal_generation: succession.seal_generation,
            seal_observation_sequence: succession.seal_observation_sequence,
            acknowledgement_observation_sequence: succession.acknowledgement_observation_sequence,
            certification: succession.certification.clone(),
            acknowledged_at: succession.acknowledged_at,
        },
    )?;
    Ok(RuntimeValidatedCompactPendingDrainSuccessionV2 {
        key: successor.key,
        drain_intent_digest: successor.drain_intent_digest,
        successor_intent_revision: successor.intent_revision,
        successor_state_digest: Sha256::digest(input.successor_state_bytes).into(),
        certification: successor.acknowledgement.certification().clone(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePersistedUnclaimedPendingDrainIntentV2 {
    canonical: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimePersistedUnclaimedPendingDrainIntentV2 {
    pub fn from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        persisted_state: &str,
        state_bytes: &[u8],
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let canonical = RuntimeCanonicalDrainIntentStateV2::from_persisted(
            root,
            intent_revision,
            persisted_state,
            state_bytes,
        )?;
        if canonical.state_kind()? != RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed {
            return Err(
                RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                    field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                },
            );
        }
        Ok(Self { canonical })
    }

    pub fn canonical(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.canonical
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2 {
    source: RuntimePersistedUnclaimedPendingDrainIntentV2,
    result: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2 {
    pub fn build(
        source: RuntimePersistedUnclaimedPendingDrainIntentV2,
        input: RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2,
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let source_intent = source.canonical().intent();
        let claimed_revision = next_intent_revision(source_intent.intent_revision())?;
        let provenance =
            RuntimeRouteMutationProvenanceV2::ClosedRecovery(input.recovery_witness.clone());
        RuntimeCanonicalRouteMutationProvenanceV2::new(provenance).map_err(|_| {
            RuntimeDrainIntentCanonicalStateErrorV2::InvalidField {
                field: RuntimeDrainIntentCanonicalStateFieldV2::Provenance,
            }
        })?;
        let process_instance_id = input.recovery_witness.process_instance_id.clone();
        let seal = RuntimeDrainClaimSealWitnessV2::new(
            source_intent.key(),
            process_instance_id.clone(),
            input.seal_generation,
            None,
            input.seal_observation_sequence,
        )?;
        let claim = RuntimeDrainClaimV2::new(
            source_intent.key(),
            input.recovery_witness.gateway_owner_lease_id,
            input.recovery_witness.observed_owner_revision,
            process_instance_id,
            input.controller_id,
            input.controller_fencing_token,
            input.claim_epoch,
            input.claim_revision,
            input.claim_expires_at,
            RuntimeDrainClaimProgressV2::claimed(seal),
        )?;
        let root = persisted_root_from_intent(source_intent)?;
        let result =
            RuntimeDrainIntentV2::pending_from_persisted(&root, claimed_revision, Some(claim))?;
        Ok(Self {
            source,
            result: RuntimeCanonicalDrainIntentStateV2::from_intent(result)?,
        })
    }

    pub fn source(&self) -> &RuntimePersistedUnclaimedPendingDrainIntentV2 {
        &self.source
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.result
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePersistedRouteAbsenceCandidateDrainIntentV2 {
    canonical: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimePersistedRouteAbsenceCandidateDrainIntentV2 {
    pub fn from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        persisted_state: &str,
        state_bytes: &[u8],
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let canonical = RuntimeCanonicalDrainIntentStateV2::from_persisted(
            root,
            intent_revision,
            persisted_state,
            state_bytes,
        )?;
        let candidate = match canonical.state_kind()? {
            RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed => canonical
                .intent()
                .state()
                .pending_claim()
                .is_some_and(|claim| claim.progress().seal().expected_route().is_none()),
            RuntimeDrainIntentCanonicalStateKindV2::PendingRefenced => true,
            RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed
            | RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
            | RuntimeDrainIntentCanonicalStateKindV2::Consumed
            | RuntimeDrainIntentCanonicalStateKindV2::Cancelled => false,
        };
        if !candidate {
            return Err(
                RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                    field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                },
            );
        }
        Ok(Self { canonical })
    }

    pub fn canonical(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.canonical
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2 {
    canonical: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2 {
    pub fn from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        persisted_state: &str,
        state_bytes: &[u8],
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let canonical = RuntimeCanonicalDrainIntentStateV2::from_persisted(
            root,
            intent_revision,
            persisted_state,
            state_bytes,
        )?;
        let candidate = canonical.state_kind()?
            == RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed
            && canonical
                .intent()
                .state()
                .pending_claim()
                .is_some_and(|claim| claim.progress().seal().expected_route().is_none());
        if !candidate {
            return Err(
                RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                    field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                },
            );
        }
        Ok(Self { canonical })
    }

    pub fn canonical(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.canonical
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2 {
    source: RuntimePersistedRouteAbsenceCandidateDrainIntentV2,
    result: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2 {
    pub fn build(
        source: RuntimePersistedRouteAbsenceCandidateDrainIntentV2,
        input: RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2,
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let source_intent = source.canonical().intent();
        let claim = source_intent.state().pending_claim().cloned().ok_or(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
            },
        )?;
        let expected_route = match claim.progress().kind() {
            RuntimeDrainClaimProgressKindV2::Claimed => None,
            RuntimeDrainClaimProgressKindV2::Refenced => {
                Some(claim.progress().removal_target().cloned().ok_or(
                    RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                        field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                    },
                )?)
            }
        };
        let result_revision = next_intent_revision(source_intent.intent_revision())?;
        let provenance = RuntimeRouteMutationProvenanceV2::ClosedRecovery(input.recovery_witness);
        let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
            source_intent.key(),
            claim,
            expected_route,
            provenance,
            input.acknowledgement_observation_sequence,
            input.certification,
            input.acknowledged_at,
        )?;
        let root = persisted_root_from_intent(source_intent)?;
        let result = RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
            &root,
            result_revision,
            acknowledgement,
        )?;
        let acknowledgement_source =
            RuntimeDrainAcknowledgementSourceV2::from_route_absence_candidate(
                source_intent.clone(),
            )?;
        RuntimeDrainIntentReceiptV2::acknowledged(&acknowledgement_source, result.clone())?;
        Ok(Self {
            source,
            result: RuntimeCanonicalDrainIntentStateV2::from_intent(result)?,
        })
    }

    pub fn source(&self) -> &RuntimePersistedRouteAbsenceCandidateDrainIntentV2 {
        &self.source
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.result
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementTransitionV2 {
    source: RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    result: RuntimeCanonicalDrainIntentStateV2,
}

impl RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementTransitionV2 {
    pub fn build(
        source: RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
        input: RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
    ) -> Result<Self, RuntimeDrainIntentCanonicalStateErrorV2> {
        let source_intent = source.canonical().intent();
        let predecessor = source_intent.state().pending_claim().ok_or(
            RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
            },
        )?;
        let expectation = RuntimeDrainSuccessionAcknowledgementExpectationV2 {
            database_now: input.database_now,
            recovery_witness: input.recovery_witness.clone(),
            controller_id: input.controller_id.clone(),
            seal_generation: input.seal_generation,
            seal_observation_sequence: input.seal_observation_sequence,
            acknowledgement_observation_sequence: input.acknowledgement_observation_sequence,
            certification: input.certification.clone(),
            acknowledged_at: input.acknowledged_at,
        };
        let receipt_source =
            RuntimeDrainSuccessionAcknowledgementSourceV2::from_expired_route_absent_claimed(
                source_intent.clone(),
                expectation,
            )?;
        let provenance =
            RuntimeRouteMutationProvenanceV2::ClosedRecovery(input.recovery_witness.clone());
        RuntimeCanonicalRouteMutationProvenanceV2::new(provenance.clone()).map_err(|_| {
            RuntimeDrainIntentCanonicalStateErrorV2::InvalidField {
                field: RuntimeDrainIntentCanonicalStateFieldV2::Provenance,
            }
        })?;
        let successor_claim_revision = next_persistence_revision(
            predecessor.claim_revision(),
            RuntimeDrainIntentCanonicalStateFieldV2::ClaimRevision,
        )?;
        let successor_fence = next_controller_fence(predecessor.controller_fencing_token())?;
        let seal = RuntimeDrainClaimSealWitnessV2::new(
            source_intent.key(),
            input.recovery_witness.process_instance_id.clone(),
            input.seal_generation,
            None,
            input.seal_observation_sequence,
        )?;
        let claim = RuntimeDrainClaimV2::new(
            source_intent.key(),
            input.recovery_witness.gateway_owner_lease_id.clone(),
            input.recovery_witness.observed_owner_revision,
            input.recovery_witness.process_instance_id,
            input.controller_id,
            successor_fence,
            input.recovery_witness.recovery_generation,
            successor_claim_revision,
            input.recovery_witness.owner_expires_at,
            RuntimeDrainClaimProgressV2::claimed(seal),
        )?;
        let acknowledgement = RuntimeRouteAbsentAcknowledgementV2::new(
            source_intent.key(),
            claim,
            None,
            provenance,
            input.acknowledgement_observation_sequence,
            input.certification,
            input.acknowledged_at,
        )?;
        let root = persisted_root_from_intent(source_intent)?;
        let result_revision = next_intent_revision(source_intent.intent_revision())?;
        let result = RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
            &root,
            result_revision,
            acknowledgement,
        )?;
        RuntimeDrainIntentReceiptV2::succession_acknowledged(&receipt_source, result.clone())?;
        Ok(Self {
            source,
            result: RuntimeCanonicalDrainIntentStateV2::from_intent(result)?,
        })
    }

    pub fn source(&self) -> &RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2 {
        &self.source
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.result
    }
}

fn next_intent_revision(
    current: NonZeroU64,
) -> Result<NonZeroU64, RuntimeDrainIntentCanonicalStateErrorV2> {
    let next = current
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })?;
    if next.get() > i64::MAX as u64 {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentCanonicalStateFieldV2::IntentRevision,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        });
    }
    Ok(next)
}

fn next_persistence_revision(
    current: NonZeroU64,
    field: RuntimeDrainIntentCanonicalStateFieldV2,
) -> Result<NonZeroU64, RuntimeDrainIntentCanonicalStateErrorV2> {
    let next = current
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .filter(|value| value.get() <= i64::MAX as u64)
        .ok_or(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })?;
    Ok(next)
}

fn next_controller_fence(
    current: FencingToken,
) -> Result<FencingToken, RuntimeDrainIntentCanonicalStateErrorV2> {
    let next =
        current.next().map_err(
            |_| RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
                field: RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
                reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
            },
        )?;
    if next.get() > i64::MAX as u64 {
        return Err(RuntimeDrainIntentCanonicalStateErrorV2::CanonicalValue {
            field: RuntimeDrainIntentCanonicalStateFieldV2::ControllerFencingToken,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        });
    }
    Ok(next)
}

fn canonical_state_kind(
    intent: &RuntimeDrainIntentV2,
) -> Result<RuntimeDrainIntentCanonicalStateKindV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    match intent.state().kind() {
        RuntimeDrainIntentStateKindV2::Pending => match intent.state().pending_claim() {
            None => Ok(RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed),
            Some(claim) => match claim.progress().kind() {
                RuntimeDrainClaimProgressKindV2::Claimed => {
                    Ok(RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed)
                }
                RuntimeDrainClaimProgressKindV2::Refenced => {
                    Ok(RuntimeDrainIntentCanonicalStateKindV2::PendingRefenced)
                }
            },
        },
        RuntimeDrainIntentStateKindV2::RouteAbsentAcknowledged => {
            if intent.state().acknowledgement().is_none() {
                return Err(
                    RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
                        field: RuntimeDrainIntentCanonicalStateCorrelationV2::PendingProgress,
                    },
                );
            }
            Ok(RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged)
        }
        RuntimeDrainIntentStateKindV2::Consumed => {
            Ok(RuntimeDrainIntentCanonicalStateKindV2::Consumed)
        }
        RuntimeDrainIntentStateKindV2::Cancelled => {
            Ok(RuntimeDrainIntentCanonicalStateKindV2::Cancelled)
        }
    }
}

fn persisted_root_from_intent(
    intent: &RuntimeDrainIntentV2,
) -> Result<RuntimePersistedProductDrainRootV2, RuntimeDrainIntentCanonicalStateErrorV2> {
    let key = intent.key();
    RuntimePersistedProductDrainRootV2::from_persisted(
        key.scope.clone(),
        key.expected_revision,
        &key.product_operation_id,
        key.scope.clone(),
        key.slot.clone(),
        key.expected_revision,
        &key.intent_id,
        &key.expected_target,
        intent.product_mutation_request_bytes(),
        intent.product_mutation_digest(),
        intent.drain_intent_request_bytes(),
        intent.drain_intent_digest(),
    )
    .map_err(
        |_| RuntimeDrainIntentCanonicalStateErrorV2::CorrelationMismatch {
            field: RuntimeDrainIntentCanonicalStateCorrelationV2::ImmutableRoot,
        },
    )
}
