mod wire;

#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::{ControllerId, FencingToken};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeCanonicalRouteMutationProvenanceV2, RuntimeCanonicalValueErrorV2,
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeDrainAcknowledgementSourceV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimErrorV2,
    RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimProgressV2, RuntimeDrainClaimSealWitnessV2,
    RuntimeDrainClaimV2, RuntimeDrainIntentReceiptErrorV2, RuntimeDrainIntentReceiptV2,
    RuntimeDrainIntentStateErrorV2, RuntimeDrainIntentStateKindV2, RuntimeDrainIntentV2,
    RuntimePersistedProductDrainRootV2, RuntimeRouteAbsentAcknowledgementV2,
    RuntimeRouteMutationProvenanceV2,
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
