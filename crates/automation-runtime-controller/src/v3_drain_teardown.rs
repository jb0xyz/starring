mod wire;

#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::{
    ControllerId, DeploymentRevision, FencingToken, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::v2_canonical_value::{RuntimePersistenceU64V2, RuntimeUnixMicrosecondsV2};
use crate::v2_drain_claim::validate_drain_claim_for_key;
use crate::{
    RuntimeCanonicalDrainIntentStateV2, RuntimeCanonicalRouteMutationProvenanceV2,
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
    RuntimeDrainCertificationResolutionKindV2, RuntimeDrainCertificationResolutionV2,
    RuntimeDrainClaimErrorV2, RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimProgressV2,
    RuntimeDrainClaimSealWitnessV2, RuntimeDrainClaimV2, RuntimeDrainIntentCanonicalStateErrorV2,
    RuntimeDrainIntentDigestV2, RuntimeDrainIntentKeyV2, RuntimeExactLocalRouteIdentityV2,
    RuntimePersistedProductDrainRootV2, RuntimePersistedRefencedPendingDrainIntentV2,
    RuntimePersistedRoutedClaimedPendingDrainIntentV2, RuntimeRouteMutationProvenanceV2,
    RuntimeServingIdentityV2,
};

const DRAIN_INTENT_STATE_MAX_OCTETS_V3: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePreviousProcessDrainProgressV3 {
    RoutedClaimed,
    Refenced,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeDrainCanonicalStateDigestV3(String);

impl RuntimeDrainCanonicalStateDigestV3 {
    pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        parse_digest(value.into()).map(Self)
    }

    pub fn from_state_bytes(state_bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(state_bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeDrainActionDigestV3(String);

impl RuntimeDrainActionDigestV3 {
    pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        parse_digest(value.into()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePreviousProcessDrainCertificationResolutionKindV3 {
    NoOperationReserved,
    NoAttestationForReservedOperation,
    CommittedAndDisconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimePreviousProcessDrainCertificationResolutionStateV3 {
    NoOperationReserved,
    NoAttestationForReservedOperation {
        operation_id: RuntimeCertificationOperationIdV2,
        intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    },
    CommittedAndDisconnected {
        operation_id: RuntimeCertificationOperationIdV2,
        serving_identity: Box<RuntimeServingIdentityV2>,
        disconnected_revision: NonZeroU64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousProcessDrainCertificationResolutionV3 {
    state: RuntimePreviousProcessDrainCertificationResolutionStateV3,
}

impl RuntimePreviousProcessDrainCertificationResolutionV3 {
    pub fn from_predecessor(
        key: &RuntimeDrainIntentKeyV2,
        predecessor_claim: &RuntimeDrainClaimV2,
        resolution: RuntimeDrainCertificationResolutionV2,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        match resolution.kind() {
            RuntimeDrainCertificationResolutionKindV2::NoOperationReserved => Ok(Self {
                state:
                    RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved,
            }),
            RuntimeDrainCertificationResolutionKindV2::NoAttestationForReservedOperation => {
                let operation_id = resolution
                    .operation_id()
                    .cloned()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?;
                let intent_fingerprint = resolution
                    .intent_fingerprint()
                    .cloned()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?;
                Ok(Self {
                    state: RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                        operation_id,
                        intent_fingerprint,
                    },
                })
            }
            RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected => {
                let operation_id = resolution
                    .operation_id()
                    .cloned()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?;
                let serving_identity = resolution
                    .serving_identity()
                    .cloned()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?;
                let disconnected_revision = resolution
                    .disconnected_revision()
                    .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?;
                let reconstructed =
                    RuntimeDrainCertificationResolutionV2::committed_and_disconnected(
                        key,
                        predecessor_claim,
                        operation_id.clone(),
                        serving_identity.clone(),
                        disconnected_revision,
                    )?;
                if reconstructed != resolution {
                    return Err(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch);
                }
                Ok(Self {
                    state: RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                        operation_id,
                        serving_identity: Box::new(serving_identity),
                        disconnected_revision,
                    },
                })
            }
        }
    }

    pub fn kind(&self) -> RuntimePreviousProcessDrainCertificationResolutionKindV3 {
        match self.state {
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved => {
                RuntimePreviousProcessDrainCertificationResolutionKindV3::NoOperationReserved
            }
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                ..
            } => RuntimePreviousProcessDrainCertificationResolutionKindV3::NoAttestationForReservedOperation,
            RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                ..
            } => RuntimePreviousProcessDrainCertificationResolutionKindV3::CommittedAndDisconnected,
        }
    }

    pub fn operation_id(&self) -> Option<&RuntimeCertificationOperationIdV2> {
        match &self.state {
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved => None,
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                operation_id,
                ..
            }
            | RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                operation_id,
                ..
            } => Some(operation_id),
        }
    }

    pub fn intent_fingerprint(&self) -> Option<&RuntimeCertificationIntentFingerprintV2> {
        match &self.state {
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                intent_fingerprint,
                ..
            } => Some(intent_fingerprint),
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved
            | RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                ..
            } => None,
        }
    }

    pub fn serving_identity(&self) -> Option<&RuntimeServingIdentityV2> {
        match &self.state {
            RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                serving_identity,
                ..
            } => Some(serving_identity),
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved
            | RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                ..
            } => None,
        }
    }

    pub fn disconnected_revision(&self) -> Option<NonZeroU64> {
        match self.state {
            RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                disconnected_revision,
                ..
            } => Some(disconnected_revision),
            RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved
            | RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                ..
            } => None,
        }
    }

    fn from_decoded_no_operation_reserved() -> Self {
        Self {
            state: RuntimePreviousProcessDrainCertificationResolutionStateV3::NoOperationReserved,
        }
    }

    fn from_decoded_no_attestation(
        operation_id: RuntimeCertificationOperationIdV2,
        intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    ) -> Self {
        Self {
            state: RuntimePreviousProcessDrainCertificationResolutionStateV3::NoAttestationForReservedOperation {
                operation_id,
                intent_fingerprint,
            },
        }
    }

    fn from_decoded_committed(
        operation_id: RuntimeCertificationOperationIdV2,
        serving_identity: RuntimeServingIdentityV2,
        disconnected_revision: NonZeroU64,
    ) -> Self {
        Self {
            state: RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
                operation_id,
                serving_identity: Box::new(serving_identity),
                disconnected_revision,
            },
        }
    }

    fn validate_for_basis(
        &self,
        key: &RuntimeDrainIntentKeyV2,
        basis: &RuntimePreviousProcessRouteAbsenceBasisV3,
    ) -> Result<(), RuntimeDrainTeardownCanonicalErrorV3> {
        if let RuntimePreviousProcessDrainCertificationResolutionStateV3::CommittedAndDisconnected {
            operation_id,
            serving_identity,
            disconnected_revision,
        } = &self.state
        {
            let expected_disconnected_revision = serving_identity
                .revision
                .get()
                .checked_add(1)
                .and_then(NonZeroU64::new)
                .filter(|value| value.get() <= i64::MAX as u64)
                .ok_or(RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch)?;
            if serving_identity.scope != key.scope
                || &serving_identity.operation_id != operation_id
                || serving_identity.process_identity != basis.route_identity
                || serving_identity.process_identity.target != key.expected_target
                || *disconnected_revision != expected_disconnected_revision
                || RuntimePersistenceU64V2::from_non_zero(serving_identity.lease_epoch).is_err()
                || RuntimePersistenceU64V2::from_non_zero(serving_identity.revision).is_err()
            {
                return Err(
                    RuntimeDrainTeardownCanonicalErrorV3::CertificationMismatch,
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousProcessRouteAbsenceBasisV3 {
    predecessor_intent_revision: NonZeroU64,
    predecessor_state_digest: RuntimeDrainCanonicalStateDigestV3,
    predecessor_progress: RuntimePreviousProcessDrainProgressV3,
    route_identity: RuntimeProcessIdentityV1,
    route_incarnation: NonZeroU64,
    source_route_fence: FencingToken,
    possible_route_fence_ceiling: FencingToken,
    predecessor_claim_terminal_digest: RuntimeDrainActionDigestV3,
    predecessor_refence_terminal_digest: Option<RuntimeDrainActionDigestV3>,
}

struct RuntimePreviousProcessRouteAbsenceBasisDecodedV3 {
    predecessor_intent_revision: NonZeroU64,
    predecessor_state_digest: RuntimeDrainCanonicalStateDigestV3,
    predecessor_progress: RuntimePreviousProcessDrainProgressV3,
    route_identity: RuntimeProcessIdentityV1,
    route_incarnation: NonZeroU64,
    source_route_fence: FencingToken,
    possible_route_fence_ceiling: FencingToken,
    predecessor_claim_terminal_digest: RuntimeDrainActionDigestV3,
    predecessor_refence_terminal_digest: Option<RuntimeDrainActionDigestV3>,
}

impl RuntimePreviousProcessRouteAbsenceBasisV3 {
    pub fn predecessor_intent_revision(&self) -> NonZeroU64 {
        self.predecessor_intent_revision
    }

    pub fn predecessor_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.predecessor_state_digest
    }

    pub fn predecessor_progress(&self) -> RuntimePreviousProcessDrainProgressV3 {
        self.predecessor_progress
    }

    pub fn route_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.route_identity
    }

    pub fn route_incarnation(&self) -> NonZeroU64 {
        self.route_incarnation
    }

    pub fn source_route_fence(&self) -> FencingToken {
        self.source_route_fence
    }

    pub fn possible_route_fence_ceiling(&self) -> FencingToken {
        self.possible_route_fence_ceiling
    }

    pub fn predecessor_claim_terminal_digest(&self) -> &RuntimeDrainActionDigestV3 {
        &self.predecessor_claim_terminal_digest
    }

    pub fn predecessor_refence_terminal_digest(&self) -> Option<&RuntimeDrainActionDigestV3> {
        self.predecessor_refence_terminal_digest.as_ref()
    }

    fn from_predecessor(
        source: &RuntimeCanonicalDrainIntentStateV2,
        predecessor_claim: &RuntimeDrainClaimV2,
        predecessor_progress: RuntimePreviousProcessDrainProgressV3,
        predecessor_claim_terminal_digest: RuntimeDrainActionDigestV3,
        predecessor_refence_terminal_digest: Option<RuntimeDrainActionDigestV3>,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        let route = predecessor_route(predecessor_claim, predecessor_progress)?;
        let basis = Self {
            predecessor_intent_revision: source.intent().intent_revision(),
            predecessor_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                source.state_bytes(),
            ),
            predecessor_progress,
            route_identity: route.identity.clone(),
            route_incarnation: route.route_incarnation,
            source_route_fence: route.controller_fencing_token,
            possible_route_fence_ceiling: predecessor_claim.controller_fencing_token(),
            predecessor_claim_terminal_digest,
            predecessor_refence_terminal_digest,
        };
        basis.validate()?;
        Ok(basis)
    }

    fn from_decoded(
        decoded: RuntimePreviousProcessRouteAbsenceBasisDecodedV3,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        let basis = Self {
            predecessor_intent_revision: decoded.predecessor_intent_revision,
            predecessor_state_digest: decoded.predecessor_state_digest,
            predecessor_progress: decoded.predecessor_progress,
            route_identity: decoded.route_identity,
            route_incarnation: decoded.route_incarnation,
            source_route_fence: decoded.source_route_fence,
            possible_route_fence_ceiling: decoded.possible_route_fence_ceiling,
            predecessor_claim_terminal_digest: decoded.predecessor_claim_terminal_digest,
            predecessor_refence_terminal_digest: decoded.predecessor_refence_terminal_digest,
        };
        basis.validate()?;
        Ok(basis)
    }

    fn validate(&self) -> Result<(), RuntimeDrainTeardownCanonicalErrorV3> {
        if RuntimePersistenceU64V2::from_non_zero(self.predecessor_intent_revision).is_err()
            || RuntimePersistenceU64V2::from_non_zero(self.route_incarnation).is_err()
            || RuntimePersistenceU64V2::from_u64(self.source_route_fence.get()).is_err()
            || RuntimePersistenceU64V2::from_u64(self.possible_route_fence_ceiling.get()).is_err()
            || self.source_route_fence.next() != Ok(self.possible_route_fence_ceiling)
        {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::RouteLineageMismatch);
        }
        let refence_shape_matches = match self.predecessor_progress {
            RuntimePreviousProcessDrainProgressV3::RoutedClaimed => {
                self.predecessor_refence_terminal_digest.is_none()
            }
            RuntimePreviousProcessDrainProgressV3::Refenced => self
                .predecessor_refence_terminal_digest
                .as_ref()
                .is_some_and(|digest| digest != &self.predecessor_claim_terminal_digest),
        };
        if !refence_shape_matches {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::JournalMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRouteAbsentAcknowledgementV3 {
    successor_claim: RuntimeDrainClaimV2,
    absence_basis: RuntimePreviousProcessRouteAbsenceBasisV3,
    provenance: RuntimeRouteMutationProvenanceV2,
    registry_observation_sequence: NonZeroU64,
    certification: RuntimePreviousProcessDrainCertificationResolutionV3,
    acknowledged_at: DateTime<Utc>,
}

impl RuntimeRouteAbsentAcknowledgementV3 {
    pub fn successor_claim(&self) -> &RuntimeDrainClaimV2 {
        &self.successor_claim
    }

    pub fn absence_basis(&self) -> &RuntimePreviousProcessRouteAbsenceBasisV3 {
        &self.absence_basis
    }

    pub fn provenance(&self) -> &RuntimeRouteMutationProvenanceV2 {
        &self.provenance
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }

    pub fn certification(&self) -> &RuntimePreviousProcessDrainCertificationResolutionV3 {
        &self.certification
    }

    pub fn acknowledged_at(&self) -> DateTime<Utc> {
        self.acknowledged_at
    }

    fn new(
        key: &RuntimeDrainIntentKeyV2,
        successor_claim: RuntimeDrainClaimV2,
        absence_basis: RuntimePreviousProcessRouteAbsenceBasisV3,
        provenance: RuntimeRouteMutationProvenanceV2,
        registry_observation_sequence: NonZeroU64,
        certification: RuntimePreviousProcessDrainCertificationResolutionV3,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        validate_drain_claim_for_key(&successor_claim, key)?;
        let witness = match &provenance {
            RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => witness,
            RuntimeRouteMutationProvenanceV2::Ordinary { .. }
            | RuntimeRouteMutationProvenanceV2::Shutdown(_) => {
                return Err(RuntimeDrainTeardownCanonicalErrorV3::ProvenanceMismatch);
            }
        };
        validate_closed_recovery_witness(witness)?;
        let expected_claim_revision = match absence_basis.predecessor_progress {
            RuntimePreviousProcessDrainProgressV3::RoutedClaimed => 2,
            RuntimePreviousProcessDrainProgressV3::Refenced => 3,
        };
        if successor_claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed
            || successor_claim.progress().seal().expected_route().is_some()
            || successor_claim.claim_revision().get() != expected_claim_revision
            || absence_basis.route_identity.target != key.expected_target
            || absence_basis.route_identity.process_instance_id
                == *successor_claim.process_instance_id()
            || absence_basis.possible_route_fence_ceiling.next()
                != Ok(successor_claim.controller_fencing_token())
            || successor_claim.gateway_owner_lease_id() != &witness.gateway_owner_lease_id
            || successor_claim.observed_owner_revision() != witness.observed_owner_revision
            || successor_claim.process_instance_id() != &witness.process_instance_id
            || successor_claim.claim_epoch() != witness.recovery_generation
            || successor_claim.expires_at() != witness.owner_expires_at
        {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::AcknowledgementMismatch);
        }
        if RuntimePersistenceU64V2::from_non_zero(registry_observation_sequence).is_err()
            || RuntimeUnixMicrosecondsV2::from_datetime(acknowledged_at).is_err()
        {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::CanonicalValue);
        }
        certification.validate_for_basis(key, &absence_basis)?;
        Ok(Self {
            successor_claim,
            absence_basis,
            provenance,
            registry_observation_sequence,
            certification,
            acknowledged_at,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentCanonicalStateKindV3 {
    RouteAbsentAcknowledged,
    Consumed,
    Cancelled,
}

impl RuntimeDrainIntentCanonicalStateKindV3 {
    pub fn persisted_state(self) -> &'static str {
        match self {
            Self::RouteAbsentAcknowledged => "route_absent_acknowledged",
            Self::Consumed => "consumed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeDrainIntentCanonicalStateValueV3 {
    RouteAbsentAcknowledged {
        acknowledgement: Box<RuntimeRouteAbsentAcknowledgementV3>,
    },
    Consumed {
        resulting_revision: DeploymentRevision,
        consumed_at: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCanonicalDrainIntentStateV3 {
    key: RuntimeDrainIntentKeyV2,
    drain_intent_digest: RuntimeDrainIntentDigestV2,
    intent_revision: NonZeroU64,
    state: RuntimeDrainIntentCanonicalStateValueV3,
    state_bytes: Box<[u8]>,
}

impl RuntimeCanonicalDrainIntentStateV3 {
    pub fn from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        persisted_state: &str,
        state_bytes: &[u8],
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        let canonical = wire::decode_state(root, intent_revision, state_bytes)?;
        if canonical.persisted_state() != persisted_state {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::PersistedStateMismatch);
        }
        Ok(canonical)
    }

    pub fn consumed_from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        resulting_revision: DeploymentRevision,
        consumed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        Self::build(
            root.canonical().drain_preimage().key.clone(),
            root.canonical().drain_intent_digest().clone(),
            intent_revision,
            RuntimeDrainIntentCanonicalStateValueV3::Consumed {
                resulting_revision,
                consumed_at,
            },
        )
    }

    pub fn cancelled_from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        cancelled_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        Self::build(
            root.canonical().drain_preimage().key.clone(),
            root.canonical().drain_intent_digest().clone(),
            intent_revision,
            RuntimeDrainIntentCanonicalStateValueV3::Cancelled { cancelled_at },
        )
    }

    pub fn key(&self) -> &RuntimeDrainIntentKeyV2 {
        &self.key
    }

    pub fn drain_intent_digest(&self) -> &RuntimeDrainIntentDigestV2 {
        &self.drain_intent_digest
    }

    pub fn intent_revision(&self) -> NonZeroU64 {
        self.intent_revision
    }

    pub fn state_kind(&self) -> RuntimeDrainIntentCanonicalStateKindV3 {
        match self.state {
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged { .. } => {
                RuntimeDrainIntentCanonicalStateKindV3::RouteAbsentAcknowledged
            }
            RuntimeDrainIntentCanonicalStateValueV3::Consumed { .. } => {
                RuntimeDrainIntentCanonicalStateKindV3::Consumed
            }
            RuntimeDrainIntentCanonicalStateValueV3::Cancelled { .. } => {
                RuntimeDrainIntentCanonicalStateKindV3::Cancelled
            }
        }
    }

    pub fn persisted_state(&self) -> &'static str {
        self.state_kind().persisted_state()
    }

    pub fn acknowledgement(&self) -> Option<&RuntimeRouteAbsentAcknowledgementV3> {
        match &self.state {
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged {
                acknowledgement,
            } => Some(acknowledgement),
            RuntimeDrainIntentCanonicalStateValueV3::Consumed { .. }
            | RuntimeDrainIntentCanonicalStateValueV3::Cancelled { .. } => None,
        }
    }

    pub fn resulting_revision(&self) -> Option<DeploymentRevision> {
        match self.state {
            RuntimeDrainIntentCanonicalStateValueV3::Consumed {
                resulting_revision, ..
            } => Some(resulting_revision),
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentCanonicalStateValueV3::Cancelled { .. } => None,
        }
    }

    pub fn consumed_at(&self) -> Option<DateTime<Utc>> {
        match self.state {
            RuntimeDrainIntentCanonicalStateValueV3::Consumed { consumed_at, .. } => {
                Some(consumed_at)
            }
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentCanonicalStateValueV3::Cancelled { .. } => None,
        }
    }

    pub fn cancelled_at(&self) -> Option<DateTime<Utc>> {
        match self.state {
            RuntimeDrainIntentCanonicalStateValueV3::Cancelled { cancelled_at } => {
                Some(cancelled_at)
            }
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentCanonicalStateValueV3::Consumed { .. } => None,
        }
    }

    pub fn state_bytes(&self) -> &[u8] {
        &self.state_bytes
    }

    fn route_absent_acknowledged(
        key: RuntimeDrainIntentKeyV2,
        drain_intent_digest: RuntimeDrainIntentDigestV2,
        intent_revision: NonZeroU64,
        acknowledgement: RuntimeRouteAbsentAcknowledgementV3,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        Self::build(
            key,
            drain_intent_digest,
            intent_revision,
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged {
                acknowledgement: Box::new(acknowledgement),
            },
        )
    }

    fn build(
        key: RuntimeDrainIntentKeyV2,
        drain_intent_digest: RuntimeDrainIntentDigestV2,
        intent_revision: NonZeroU64,
        state: RuntimeDrainIntentCanonicalStateValueV3,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        if RuntimePersistenceU64V2::from_non_zero(intent_revision).is_err() {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::CanonicalValue);
        }
        match &state {
            RuntimeDrainIntentCanonicalStateValueV3::RouteAbsentAcknowledged {
                acknowledgement,
            } => {
                let expected_intent_revision = next_persistence_revision(
                    acknowledgement.absence_basis.predecessor_intent_revision,
                    RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionOverflow,
                )?;
                if intent_revision != expected_intent_revision {
                    return Err(RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionMismatch);
                }
                acknowledgement
                    .certification
                    .validate_for_basis(&key, &acknowledgement.absence_basis)?;
            }
            RuntimeDrainIntentCanonicalStateValueV3::Consumed {
                resulting_revision,
                consumed_at,
            } => {
                if RuntimePersistenceU64V2::from_u64(resulting_revision.get()).is_err()
                    || RuntimeUnixMicrosecondsV2::from_datetime(*consumed_at).is_err()
                {
                    return Err(RuntimeDrainTeardownCanonicalErrorV3::CanonicalValue);
                }
            }
            RuntimeDrainIntentCanonicalStateValueV3::Cancelled { cancelled_at } => {
                if RuntimeUnixMicrosecondsV2::from_datetime(*cancelled_at).is_err() {
                    return Err(RuntimeDrainTeardownCanonicalErrorV3::CanonicalValue);
                }
            }
        }
        let mut canonical = Self {
            key,
            drain_intent_digest,
            intent_revision,
            state,
            state_bytes: Box::new([]),
        };
        canonical.state_bytes = wire::encode_state(&canonical)?.into_boxed_slice();
        Ok(canonical)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousProcessDrainTeardownSuccessionInputV3 {
    pub database_now: DateTime<Utc>,
    pub recovery_witness: crate::RuntimeClosedRecoveryRouteWitnessV2,
    pub controller_id: ControllerId,
    pub seal_generation: NonZeroU64,
    pub seal_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub predecessor_claim_terminal_digest: RuntimeDrainActionDigestV3,
    pub predecessor_refence_terminal_digest: Option<RuntimeDrainActionDigestV3>,
    pub certification: RuntimeDrainCertificationResolutionV2,
    pub acknowledged_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousProcessDrainTeardownSuccessionTransitionV3 {
    source: RuntimeCanonicalDrainIntentStateV2,
    result: RuntimeCanonicalDrainIntentStateV3,
}

impl RuntimePreviousProcessDrainTeardownSuccessionTransitionV3 {
    pub fn from_routed_claimed(
        source: RuntimePersistedRoutedClaimedPendingDrainIntentV2,
        input: RuntimePreviousProcessDrainTeardownSuccessionInputV3,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        Self::build(
            source.canonical().clone(),
            RuntimePreviousProcessDrainProgressV3::RoutedClaimed,
            input,
        )
    }

    pub fn from_refenced(
        source: RuntimePersistedRefencedPendingDrainIntentV2,
        input: RuntimePreviousProcessDrainTeardownSuccessionInputV3,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        Self::build(
            source.canonical().clone(),
            RuntimePreviousProcessDrainProgressV3::Refenced,
            input,
        )
    }

    pub fn source(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.source
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV3 {
        &self.result
    }

    pub fn verify_persisted_result(
        &self,
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        persisted_state: &str,
        state_bytes: &[u8],
    ) -> Result<RuntimeCanonicalDrainIntentStateV3, RuntimeDrainTeardownCanonicalErrorV3> {
        let persisted = RuntimeCanonicalDrainIntentStateV3::from_persisted(
            root,
            intent_revision,
            persisted_state,
            state_bytes,
        )?;
        if persisted != self.result {
            return Err(RuntimeDrainTeardownCanonicalErrorV3::PersistedResultMismatch);
        }
        Ok(persisted)
    }

    fn build(
        source: RuntimeCanonicalDrainIntentStateV2,
        predecessor_progress: RuntimePreviousProcessDrainProgressV3,
        input: RuntimePreviousProcessDrainTeardownSuccessionInputV3,
    ) -> Result<Self, RuntimeDrainTeardownCanonicalErrorV3> {
        let predecessor_claim = source
            .intent()
            .state()
            .pending_claim()
            .ok_or(RuntimeDrainTeardownCanonicalErrorV3::SourceStateMismatch)?;
        validate_succession_evidence(predecessor_claim, &input)?;
        let absence_basis = RuntimePreviousProcessRouteAbsenceBasisV3::from_predecessor(
            &source,
            predecessor_claim,
            predecessor_progress,
            input.predecessor_claim_terminal_digest,
            input.predecessor_refence_terminal_digest,
        )?;
        let certification = RuntimePreviousProcessDrainCertificationResolutionV3::from_predecessor(
            source.intent().key(),
            predecessor_claim,
            input.certification,
        )?;
        certification.validate_for_basis(source.intent().key(), &absence_basis)?;
        let successor_claim_revision = next_persistence_revision(
            predecessor_claim.claim_revision(),
            RuntimeDrainTeardownCanonicalErrorV3::ClaimRevisionOverflow,
        )?;
        let successor_fence = next_fence(absence_basis.possible_route_fence_ceiling)?;
        let witness = &input.recovery_witness;
        let seal = RuntimeDrainClaimSealWitnessV2::new(
            source.intent().key(),
            witness.process_instance_id.clone(),
            input.seal_generation,
            None,
            input.seal_observation_sequence,
        )?;
        let successor_claim = RuntimeDrainClaimV2::new(
            source.intent().key(),
            witness.gateway_owner_lease_id.clone(),
            witness.observed_owner_revision,
            witness.process_instance_id.clone(),
            input.controller_id,
            successor_fence,
            witness.recovery_generation,
            successor_claim_revision,
            witness.owner_expires_at,
            RuntimeDrainClaimProgressV2::claimed(seal),
        )?;
        let provenance = RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness.clone());
        let acknowledgement = RuntimeRouteAbsentAcknowledgementV3::new(
            source.intent().key(),
            successor_claim,
            absence_basis,
            provenance,
            input.registry_observation_sequence,
            certification,
            input.acknowledged_at,
        )?;
        let result = RuntimeCanonicalDrainIntentStateV3::route_absent_acknowledged(
            source.intent().key().clone(),
            source.intent().drain_intent_digest().clone(),
            next_persistence_revision(
                source.intent().intent_revision(),
                RuntimeDrainTeardownCanonicalErrorV3::IntentRevisionOverflow,
            )?,
            acknowledgement,
        )?;
        Ok(Self { source, result })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDrainTeardownCanonicalErrorV3 {
    #[error("runtime V3 drain-teardown digest is invalid")]
    InvalidDigest,
    #[error("runtime V3 drain-teardown source state does not match")]
    SourceStateMismatch,
    #[error("runtime V3 drain-teardown durable route evidence is missing")]
    MissingRouteEvidence,
    #[error("runtime V3 drain-teardown route lineage does not match")]
    RouteLineageMismatch,
    #[error("runtime V3 drain-teardown journal lineage does not match")]
    JournalMismatch,
    #[error("runtime V3 drain-teardown predecessor is not expired at database time")]
    PredecessorNotExpired,
    #[error("runtime V3 drain-teardown database time is not canonical")]
    DatabaseTimeInvalid,
    #[error("runtime V3 drain-teardown process is not distinct from its predecessor")]
    ProcessNotDistinct,
    #[error("runtime V3 drain-teardown owner identity does not match")]
    OwnerMismatch,
    #[error("runtime V3 drain-teardown gateway shard changed")]
    ShardMismatch,
    #[error("runtime V3 drain-teardown owner epoch is not strictly newer")]
    OwnerEpochNotNewer,
    #[error("runtime V3 drain-teardown current owner is expired at database time")]
    OwnerExpired,
    #[error("runtime V3 drain-teardown intent revision overflowed")]
    IntentRevisionOverflow,
    #[error("runtime V3 drain-teardown intent revision does not follow its predecessor")]
    IntentRevisionMismatch,
    #[error("runtime V3 drain-teardown claim revision overflowed")]
    ClaimRevisionOverflow,
    #[error("runtime V3 drain-teardown controller fence overflowed")]
    FenceOverflow,
    #[error("runtime V3 drain-teardown provenance does not match")]
    ProvenanceMismatch,
    #[error("runtime V3 drain-teardown acknowledgement does not match")]
    AcknowledgementMismatch,
    #[error("runtime V3 drain-teardown certification does not match its predecessor")]
    CertificationMismatch,
    #[error("runtime V3 drain-teardown canonical scalar is invalid")]
    CanonicalValue,
    #[error("runtime V3 drain-teardown payload decoding failed")]
    Decoding,
    #[error("runtime V3 drain-teardown payload format version is unsupported")]
    UnsupportedFormatVersion,
    #[error("runtime V3 drain-teardown payload has a noncanonical representation")]
    NonCanonicalEncoding,
    #[error("runtime V3 drain-teardown immutable root does not match")]
    ImmutableRootMismatch,
    #[error("runtime V3 drain-teardown persisted state does not match")]
    PersistedStateMismatch,
    #[error("runtime V3 drain-teardown persisted result does not match")]
    PersistedResultMismatch,
    #[error(transparent)]
    CanonicalV2(#[from] RuntimeDrainIntentCanonicalStateErrorV2),
    #[error(transparent)]
    Claim(#[from] RuntimeDrainClaimErrorV2),
}

fn predecessor_route(
    predecessor_claim: &RuntimeDrainClaimV2,
    predecessor_progress: RuntimePreviousProcessDrainProgressV3,
) -> Result<&RuntimeExactLocalRouteIdentityV2, RuntimeDrainTeardownCanonicalErrorV3> {
    match predecessor_progress {
        RuntimePreviousProcessDrainProgressV3::RoutedClaimed => {
            if predecessor_claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed {
                return Err(RuntimeDrainTeardownCanonicalErrorV3::SourceStateMismatch);
            }
            predecessor_claim
                .progress()
                .seal()
                .expected_route()
                .ok_or(RuntimeDrainTeardownCanonicalErrorV3::MissingRouteEvidence)
        }
        RuntimePreviousProcessDrainProgressV3::Refenced => {
            if predecessor_claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Refenced {
                return Err(RuntimeDrainTeardownCanonicalErrorV3::SourceStateMismatch);
            }
            let old_route = predecessor_claim
                .progress()
                .old_route()
                .ok_or(RuntimeDrainTeardownCanonicalErrorV3::MissingRouteEvidence)?;
            let removal_target = predecessor_claim
                .progress()
                .removal_target()
                .ok_or(RuntimeDrainTeardownCanonicalErrorV3::MissingRouteEvidence)?;
            if predecessor_claim.progress().seal().expected_route() != Some(old_route)
                || old_route.identity != removal_target.identity
                || old_route.route_incarnation != removal_target.route_incarnation
                || removal_target.controller_fencing_token
                    != predecessor_claim.controller_fencing_token()
            {
                return Err(RuntimeDrainTeardownCanonicalErrorV3::RouteLineageMismatch);
            }
            Ok(old_route)
        }
    }
}

fn validate_succession_evidence(
    predecessor_claim: &RuntimeDrainClaimV2,
    input: &RuntimePreviousProcessDrainTeardownSuccessionInputV3,
) -> Result<(), RuntimeDrainTeardownCanonicalErrorV3> {
    if RuntimeUnixMicrosecondsV2::from_datetime(input.database_now).is_err() {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::DatabaseTimeInvalid);
    }
    if input.database_now < predecessor_claim.expires_at() {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::PredecessorNotExpired);
    }
    let witness = &input.recovery_witness;
    if witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::OwnerMismatch);
    }
    validate_closed_recovery_witness(witness)?;
    if predecessor_claim.process_instance_id() == &witness.process_instance_id {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::ProcessNotDistinct);
    }
    if predecessor_claim.gateway_owner_lease_id().gateway_shard_id
        != witness.gateway_owner_lease_id.gateway_shard_id
    {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::ShardMismatch);
    }
    if witness.gateway_owner_lease_id.lease_epoch
        <= predecessor_claim.gateway_owner_lease_id().lease_epoch
    {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::OwnerEpochNotNewer);
    }
    if input.database_now >= witness.owner_expires_at {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::OwnerExpired);
    }
    Ok(())
}

fn validate_closed_recovery_witness(
    witness: &crate::RuntimeClosedRecoveryRouteWitnessV2,
) -> Result<(), RuntimeDrainTeardownCanonicalErrorV3> {
    RuntimeCanonicalRouteMutationProvenanceV2::new(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness.clone()),
    )
    .map_err(|_| RuntimeDrainTeardownCanonicalErrorV3::ProvenanceMismatch)?;
    if witness
        .originating_emergency_generation
        .get()
        .checked_add(1)
        != Some(witness.recovery_generation.get())
        || witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id
        || witness.pause_sequence.get() <= witness.connected_event_sequence.get()
    {
        return Err(RuntimeDrainTeardownCanonicalErrorV3::ProvenanceMismatch);
    }
    Ok(())
}

fn next_persistence_revision(
    current: NonZeroU64,
    overflow: RuntimeDrainTeardownCanonicalErrorV3,
) -> Result<NonZeroU64, RuntimeDrainTeardownCanonicalErrorV3> {
    current
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .filter(|value| value.get() <= i64::MAX as u64)
        .ok_or(overflow)
}

fn next_fence(current: FencingToken) -> Result<FencingToken, RuntimeDrainTeardownCanonicalErrorV3> {
    current
        .next()
        .ok()
        .filter(|value| value.get() <= i64::MAX as u64)
        .ok_or(RuntimeDrainTeardownCanonicalErrorV3::FenceOverflow)
}

fn parse_digest(value: String) -> Result<String, RuntimeDrainTeardownCanonicalErrorV3> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        Err(RuntimeDrainTeardownCanonicalErrorV3::InvalidDigest)
    }
}
