#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::{
    ControllerId, FencingToken, ProcessInstanceId, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, Utc};

use crate::v2_canonical_value::{
    RuntimeDiscordSnowflakeV2, RuntimePersistenceU64V2, RuntimeUnixMicrosecondsV2,
};
use crate::{
    RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeClosedRecoveryRouteWitnessV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentKeyV2, RuntimeExactLocalRouteIdentityV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeRouteMutationProvenanceV2, RuntimeServingIdentityV2, RuntimeServingSlotV2,
    RuntimeShutdownRouteWitnessV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainClaimFieldV2 {
    ExpectedRevision,
    SealGeneration,
    SealObservationSequence,
    OwnerLeaseEpoch,
    OwnerRevision,
    ControllerFencingToken,
    ClaimEpoch,
    ClaimRevision,
    ClaimExpiresAt,
    RouteGuildId,
    RouteBindingRevision,
    RouteRuntimeGeneration,
    RouteControllerFencingToken,
    RouteIncarnation,
    RefenceObservationSequence,
    RefencedAt,
    ProvenanceInteger,
    ProvenanceOwnerExpiresAt,
    ServingBindingRevision,
    ServingRuntimeGeneration,
    ServingLeaseEpoch,
    ServingRevision,
    DisconnectedRevision,
    AcknowledgementObservationSequence,
    AcknowledgedAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDrainClaimErrorV2 {
    #[error("runtime drain-claim field {field:?} is invalid: {reason}")]
    CanonicalValue {
        field: RuntimeDrainClaimFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error("runtime drain-claim intent does not match")]
    IntentMismatch,
    #[error("runtime drain-claim slot does not match")]
    SlotMismatch,
    #[error("runtime drain-claim target does not match")]
    TargetMismatch,
    #[error("runtime drain-claim process does not match")]
    ProcessMismatch,
    #[error("runtime drain-claim owner does not match")]
    OwnerMismatch,
    #[error("runtime drain-claim expected route does not match")]
    ExpectedRouteMismatch,
    #[error("runtime drain-claim refence changed route identity")]
    RefencedRouteMismatch,
    #[error("runtime drain-claim refence fence is not strictly newer")]
    RefencedFenceNotNewer,
    #[error("runtime drain-claim refence observation does not follow its seal")]
    RefenceObservationNotAfterSeal,
    #[error("runtime drain-claim fence does not match its route progress")]
    ClaimFenceMismatch,
    #[error("runtime drain-claim provenance does not match")]
    ProvenanceMismatch,
    #[error("runtime drain certification scope does not match")]
    CertificationScopeMismatch,
    #[error("runtime drain certification operation does not match")]
    CertificationOperationMismatch,
    #[error("runtime drain certification target does not match")]
    CertificationTargetMismatch,
    #[error("runtime drain certification process does not match")]
    CertificationProcessMismatch,
    #[error("runtime drain certification route identity does not match")]
    CertificationRouteMismatch,
    #[error("runtime drain certification disconnect revision is not the exact successor")]
    CertificationDisconnectRevisionMismatch,
    #[error("runtime route-absence acknowledgement does not match claim progress")]
    AcknowledgementProgressMismatch,
    #[error("runtime route-absence observation does not follow refence progress")]
    AcknowledgementObservationNotAfterRefence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainClaimSealWitnessV2 {
    process_instance_id: ProcessInstanceId,
    slot: RuntimeServingSlotV2,
    intent_id: RuntimeDrainIntentIdV2,
    seal_generation: NonZeroU64,
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    registry_observation_sequence: NonZeroU64,
}

impl RuntimeDrainClaimSealWitnessV2 {
    pub fn new(
        key: &RuntimeDrainIntentKeyV2,
        process_instance_id: ProcessInstanceId,
        seal_generation: NonZeroU64,
        expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
        registry_observation_sequence: NonZeroU64,
    ) -> Result<Self, RuntimeDrainClaimErrorV2> {
        validate_key(key)?;
        validate_non_zero(seal_generation, RuntimeDrainClaimFieldV2::SealGeneration)?;
        validate_non_zero(
            registry_observation_sequence,
            RuntimeDrainClaimFieldV2::SealObservationSequence,
        )?;
        if let Some(route) = expected_route.as_ref() {
            validate_route(route)?;
            if route.slot() != key.slot {
                return Err(RuntimeDrainClaimErrorV2::SlotMismatch);
            }
            if route.identity.target != key.expected_target {
                return Err(RuntimeDrainClaimErrorV2::TargetMismatch);
            }
            if route.identity.process_instance_id != process_instance_id {
                return Err(RuntimeDrainClaimErrorV2::ProcessMismatch);
            }
        }
        Ok(Self {
            process_instance_id,
            slot: key.slot.clone(),
            intent_id: key.intent_id.clone(),
            seal_generation,
            expected_route,
            registry_observation_sequence,
        })
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn expected_route(&self) -> Option<&RuntimeExactLocalRouteIdentityV2> {
        self.expected_route.as_ref()
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }

    fn validate_for_key(
        &self,
        key: &RuntimeDrainIntentKeyV2,
    ) -> Result<(), RuntimeDrainClaimErrorV2> {
        if self.intent_id != key.intent_id {
            return Err(RuntimeDrainClaimErrorV2::IntentMismatch);
        }
        if self.slot != key.slot {
            return Err(RuntimeDrainClaimErrorV2::SlotMismatch);
        }
        if let Some(route) = self.expected_route.as_ref() {
            if route.identity.target != key.expected_target {
                return Err(RuntimeDrainClaimErrorV2::TargetMismatch);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainClaimProgressKindV2 {
    Claimed,
    Refenced,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeDrainClaimProgressStateV2 {
    Claimed {
        seal: RuntimeDrainClaimSealWitnessV2,
    },
    Refenced {
        seal: RuntimeDrainClaimSealWitnessV2,
        provenance: Box<RuntimeRouteMutationProvenanceV2>,
        old_route: Box<RuntimeExactLocalRouteIdentityV2>,
        removal_target: Box<RuntimeExactLocalRouteIdentityV2>,
        registry_observation_sequence: NonZeroU64,
        refenced_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainClaimProgressV2 {
    state: RuntimeDrainClaimProgressStateV2,
}

impl RuntimeDrainClaimProgressV2 {
    pub fn claimed(seal: RuntimeDrainClaimSealWitnessV2) -> Self {
        Self {
            state: RuntimeDrainClaimProgressStateV2::Claimed { seal },
        }
    }

    pub fn refenced(
        seal: RuntimeDrainClaimSealWitnessV2,
        provenance: RuntimeRouteMutationProvenanceV2,
        old_route: RuntimeExactLocalRouteIdentityV2,
        removal_target: RuntimeExactLocalRouteIdentityV2,
        registry_observation_sequence: NonZeroU64,
        refenced_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainClaimErrorV2> {
        validate_route(&old_route)?;
        validate_route(&removal_target)?;
        if seal.expected_route() != Some(&old_route) {
            return Err(RuntimeDrainClaimErrorV2::ExpectedRouteMismatch);
        }
        if old_route.identity != removal_target.identity
            || old_route.route_incarnation != removal_target.route_incarnation
        {
            return Err(RuntimeDrainClaimErrorV2::RefencedRouteMismatch);
        }
        if removal_target.controller_fencing_token <= old_route.controller_fencing_token {
            return Err(RuntimeDrainClaimErrorV2::RefencedFenceNotNewer);
        }
        validate_non_zero(
            registry_observation_sequence,
            RuntimeDrainClaimFieldV2::RefenceObservationSequence,
        )?;
        if registry_observation_sequence <= seal.registry_observation_sequence() {
            return Err(RuntimeDrainClaimErrorV2::RefenceObservationNotAfterSeal);
        }
        validate_datetime(refenced_at, RuntimeDrainClaimFieldV2::RefencedAt)?;
        validate_provenance(&provenance)?;
        validate_provenance_process(&provenance, seal.process_instance_id())?;
        Ok(Self {
            state: RuntimeDrainClaimProgressStateV2::Refenced {
                seal,
                provenance: Box::new(provenance),
                old_route: Box::new(old_route),
                removal_target: Box::new(removal_target),
                registry_observation_sequence,
                refenced_at,
            },
        })
    }

    pub fn kind(&self) -> RuntimeDrainClaimProgressKindV2 {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { .. } => {
                RuntimeDrainClaimProgressKindV2::Claimed
            }
            RuntimeDrainClaimProgressStateV2::Refenced { .. } => {
                RuntimeDrainClaimProgressKindV2::Refenced
            }
        }
    }

    pub fn seal(&self) -> &RuntimeDrainClaimSealWitnessV2 {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { seal }
            | RuntimeDrainClaimProgressStateV2::Refenced { seal, .. } => seal,
        }
    }

    pub fn provenance(&self) -> Option<&RuntimeRouteMutationProvenanceV2> {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { .. } => None,
            RuntimeDrainClaimProgressStateV2::Refenced { provenance, .. } => {
                Some(provenance.as_ref())
            }
        }
    }

    pub fn old_route(&self) -> Option<&RuntimeExactLocalRouteIdentityV2> {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { .. } => None,
            RuntimeDrainClaimProgressStateV2::Refenced { old_route, .. } => {
                Some(old_route.as_ref())
            }
        }
    }

    pub fn removal_target(&self) -> Option<&RuntimeExactLocalRouteIdentityV2> {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { .. } => None,
            RuntimeDrainClaimProgressStateV2::Refenced { removal_target, .. } => {
                Some(removal_target.as_ref())
            }
        }
    }

    pub fn registry_observation_sequence(&self) -> Option<NonZeroU64> {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { .. } => None,
            RuntimeDrainClaimProgressStateV2::Refenced {
                registry_observation_sequence,
                ..
            } => Some(*registry_observation_sequence),
        }
    }

    pub fn refenced_at(&self) -> Option<DateTime<Utc>> {
        match &self.state {
            RuntimeDrainClaimProgressStateV2::Claimed { .. } => None,
            RuntimeDrainClaimProgressStateV2::Refenced { refenced_at, .. } => Some(*refenced_at),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainClaimV2 {
    key: RuntimeDrainIntentKeyV2,
    gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    observed_owner_revision: NonZeroU64,
    process_instance_id: ProcessInstanceId,
    controller_id: ControllerId,
    controller_fencing_token: FencingToken,
    claim_epoch: NonZeroU64,
    claim_revision: NonZeroU64,
    expires_at: DateTime<Utc>,
    progress: RuntimeDrainClaimProgressV2,
}

impl RuntimeDrainClaimV2 {
    #[expect(
        clippy::too_many_arguments,
        reason = "the checked claim binds every persisted claim identity"
    )]
    pub fn new(
        key: &RuntimeDrainIntentKeyV2,
        gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
        observed_owner_revision: NonZeroU64,
        process_instance_id: ProcessInstanceId,
        controller_id: ControllerId,
        controller_fencing_token: FencingToken,
        claim_epoch: NonZeroU64,
        claim_revision: NonZeroU64,
        expires_at: DateTime<Utc>,
        progress: RuntimeDrainClaimProgressV2,
    ) -> Result<Self, RuntimeDrainClaimErrorV2> {
        validate_key(key)?;
        progress.seal().validate_for_key(key)?;
        if progress.seal().process_instance_id() != &process_instance_id
            || gateway_owner_lease_id.process_instance_id != process_instance_id
        {
            return Err(RuntimeDrainClaimErrorV2::ProcessMismatch);
        }
        validate_owner_lease(&gateway_owner_lease_id)?;
        validate_non_zero(
            observed_owner_revision,
            RuntimeDrainClaimFieldV2::OwnerRevision,
        )?;
        validate_fencing_token(
            controller_fencing_token,
            RuntimeDrainClaimFieldV2::ControllerFencingToken,
        )?;
        validate_non_zero(claim_epoch, RuntimeDrainClaimFieldV2::ClaimEpoch)?;
        validate_non_zero(claim_revision, RuntimeDrainClaimFieldV2::ClaimRevision)?;
        validate_datetime(expires_at, RuntimeDrainClaimFieldV2::ClaimExpiresAt)?;
        if let Some(old_route) = progress.seal().expected_route() {
            if controller_fencing_token <= old_route.controller_fencing_token {
                return Err(RuntimeDrainClaimErrorV2::ClaimFenceMismatch);
            }
        }
        if let Some(removal_target) = progress.removal_target() {
            if removal_target.controller_fencing_token != controller_fencing_token {
                return Err(RuntimeDrainClaimErrorV2::ClaimFenceMismatch);
            }
        }
        if let Some(provenance) = progress.provenance() {
            validate_provenance_owner(
                provenance,
                &gateway_owner_lease_id,
                observed_owner_revision,
            )?;
        }
        Ok(Self {
            key: key.clone(),
            gateway_owner_lease_id,
            observed_owner_revision,
            process_instance_id,
            controller_id,
            controller_fencing_token,
            claim_epoch,
            claim_revision,
            expires_at,
            progress,
        })
    }

    pub fn gateway_owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.gateway_owner_lease_id
    }

    pub fn observed_owner_revision(&self) -> NonZeroU64 {
        self.observed_owner_revision
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn controller_id(&self) -> &ControllerId {
        &self.controller_id
    }

    pub fn controller_fencing_token(&self) -> FencingToken {
        self.controller_fencing_token
    }

    pub fn claim_epoch(&self) -> NonZeroU64 {
        self.claim_epoch
    }

    pub fn claim_revision(&self) -> NonZeroU64 {
        self.claim_revision
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn progress(&self) -> &RuntimeDrainClaimProgressV2 {
        &self.progress
    }

    fn validate_for_key(
        &self,
        key: &RuntimeDrainIntentKeyV2,
    ) -> Result<(), RuntimeDrainClaimErrorV2> {
        if &self.key != key {
            return Err(RuntimeDrainClaimErrorV2::IntentMismatch);
        }
        self.progress.seal().validate_for_key(key)?;
        if self.progress.seal().process_instance_id() != &self.process_instance_id
            || self.gateway_owner_lease_id.process_instance_id != self.process_instance_id
        {
            return Err(RuntimeDrainClaimErrorV2::ProcessMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainCertificationResolutionKindV2 {
    NoOperationReserved,
    NoAttestationForReservedOperation,
    CommittedAndDisconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeDrainCertificationResolutionStateV2 {
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
pub struct RuntimeDrainCertificationResolutionV2 {
    state: RuntimeDrainCertificationResolutionStateV2,
}

impl RuntimeDrainCertificationResolutionV2 {
    pub fn no_operation_reserved() -> Self {
        Self {
            state: RuntimeDrainCertificationResolutionStateV2::NoOperationReserved,
        }
    }

    pub fn no_attestation_for_reserved_operation(
        operation_id: RuntimeCertificationOperationIdV2,
        intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    ) -> Self {
        Self {
            state: RuntimeDrainCertificationResolutionStateV2::NoAttestationForReservedOperation {
                operation_id,
                intent_fingerprint,
            },
        }
    }

    pub fn committed_and_disconnected(
        key: &RuntimeDrainIntentKeyV2,
        claim: &RuntimeDrainClaimV2,
        operation_id: RuntimeCertificationOperationIdV2,
        serving_identity: RuntimeServingIdentityV2,
        disconnected_revision: NonZeroU64,
    ) -> Result<Self, RuntimeDrainClaimErrorV2> {
        validate_key(key)?;
        claim.validate_for_key(key)?;
        validate_serving_identity(&serving_identity)?;
        validate_non_zero(
            disconnected_revision,
            RuntimeDrainClaimFieldV2::DisconnectedRevision,
        )?;
        validate_disconnected_revision(&serving_identity, disconnected_revision)?;
        if serving_identity.scope != key.scope {
            return Err(RuntimeDrainClaimErrorV2::CertificationScopeMismatch);
        }
        if serving_identity.operation_id != operation_id {
            return Err(RuntimeDrainClaimErrorV2::CertificationOperationMismatch);
        }
        if serving_identity.process_identity.target != key.expected_target {
            return Err(RuntimeDrainClaimErrorV2::CertificationTargetMismatch);
        }
        if serving_identity.process_identity.process_instance_id != claim.process_instance_id {
            return Err(RuntimeDrainClaimErrorV2::CertificationProcessMismatch);
        }
        if claim
            .progress()
            .seal()
            .expected_route()
            .is_some_and(|route| route.identity != serving_identity.process_identity)
        {
            return Err(RuntimeDrainClaimErrorV2::CertificationRouteMismatch);
        }
        Ok(Self {
            state: RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected {
                operation_id,
                serving_identity: Box::new(serving_identity),
                disconnected_revision,
            },
        })
    }

    pub fn kind(&self) -> RuntimeDrainCertificationResolutionKindV2 {
        match &self.state {
            RuntimeDrainCertificationResolutionStateV2::NoOperationReserved => {
                RuntimeDrainCertificationResolutionKindV2::NoOperationReserved
            }
            RuntimeDrainCertificationResolutionStateV2::NoAttestationForReservedOperation {
                ..
            } => RuntimeDrainCertificationResolutionKindV2::NoAttestationForReservedOperation,
            RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected { .. } => {
                RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
            }
        }
    }

    pub fn operation_id(&self) -> Option<&RuntimeCertificationOperationIdV2> {
        match &self.state {
            RuntimeDrainCertificationResolutionStateV2::NoOperationReserved => None,
            RuntimeDrainCertificationResolutionStateV2::NoAttestationForReservedOperation {
                operation_id,
                ..
            }
            | RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected {
                operation_id,
                ..
            } => Some(operation_id),
        }
    }

    pub fn intent_fingerprint(&self) -> Option<&RuntimeCertificationIntentFingerprintV2> {
        match &self.state {
            RuntimeDrainCertificationResolutionStateV2::NoAttestationForReservedOperation {
                intent_fingerprint,
                ..
            } => Some(intent_fingerprint),
            RuntimeDrainCertificationResolutionStateV2::NoOperationReserved
            | RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected { .. } => None,
        }
    }

    pub fn serving_identity(&self) -> Option<&RuntimeServingIdentityV2> {
        match &self.state {
            RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected {
                serving_identity,
                ..
            } => Some(serving_identity.as_ref()),
            RuntimeDrainCertificationResolutionStateV2::NoOperationReserved
            | RuntimeDrainCertificationResolutionStateV2::NoAttestationForReservedOperation {
                ..
            } => None,
        }
    }

    pub fn disconnected_revision(&self) -> Option<NonZeroU64> {
        match &self.state {
            RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected {
                disconnected_revision,
                ..
            } => Some(*disconnected_revision),
            RuntimeDrainCertificationResolutionStateV2::NoOperationReserved
            | RuntimeDrainCertificationResolutionStateV2::NoAttestationForReservedOperation {
                ..
            } => None,
        }
    }

    fn validate_for(
        &self,
        key: &RuntimeDrainIntentKeyV2,
        claim: &RuntimeDrainClaimV2,
    ) -> Result<(), RuntimeDrainClaimErrorV2> {
        if let RuntimeDrainCertificationResolutionStateV2::CommittedAndDisconnected {
            operation_id,
            serving_identity,
            disconnected_revision,
        } = &self.state
        {
            validate_serving_identity(serving_identity)?;
            let disconnected_revision = *disconnected_revision;
            validate_non_zero(
                disconnected_revision,
                RuntimeDrainClaimFieldV2::DisconnectedRevision,
            )?;
            validate_disconnected_revision(serving_identity, disconnected_revision)?;
            if serving_identity.scope != key.scope {
                return Err(RuntimeDrainClaimErrorV2::CertificationScopeMismatch);
            }
            if &serving_identity.operation_id != operation_id {
                return Err(RuntimeDrainClaimErrorV2::CertificationOperationMismatch);
            }
            if serving_identity.process_identity.target != key.expected_target {
                return Err(RuntimeDrainClaimErrorV2::CertificationTargetMismatch);
            }
            if serving_identity.process_identity.process_instance_id != claim.process_instance_id {
                return Err(RuntimeDrainClaimErrorV2::CertificationProcessMismatch);
            }
            if claim
                .progress()
                .seal()
                .expected_route()
                .is_some_and(|route| route.identity != serving_identity.process_identity)
            {
                return Err(RuntimeDrainClaimErrorV2::CertificationRouteMismatch);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRouteAbsentAcknowledgementV2 {
    claim: RuntimeDrainClaimV2,
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    provenance: RuntimeRouteMutationProvenanceV2,
    registry_observation_sequence: NonZeroU64,
    certification: RuntimeDrainCertificationResolutionV2,
    acknowledged_at: DateTime<Utc>,
}

impl RuntimeRouteAbsentAcknowledgementV2 {
    pub fn new(
        key: &RuntimeDrainIntentKeyV2,
        claim: RuntimeDrainClaimV2,
        expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
        provenance: RuntimeRouteMutationProvenanceV2,
        registry_observation_sequence: NonZeroU64,
        certification: RuntimeDrainCertificationResolutionV2,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainClaimErrorV2> {
        validate_key(key)?;
        claim.validate_for_key(key)?;
        validate_provenance(&provenance)?;
        validate_provenance_process(&provenance, claim.process_instance_id())?;
        validate_provenance_owner(
            &provenance,
            claim.gateway_owner_lease_id(),
            claim.observed_owner_revision(),
        )?;
        validate_non_zero(
            registry_observation_sequence,
            RuntimeDrainClaimFieldV2::AcknowledgementObservationSequence,
        )?;
        validate_datetime(acknowledged_at, RuntimeDrainClaimFieldV2::AcknowledgedAt)?;
        certification.validate_for(key, &claim)?;
        match claim.progress().kind() {
            RuntimeDrainClaimProgressKindV2::Claimed => {
                if claim.progress().seal().expected_route().is_some() || expected_route.is_some() {
                    return Err(RuntimeDrainClaimErrorV2::AcknowledgementProgressMismatch);
                }
            }
            RuntimeDrainClaimProgressKindV2::Refenced => {
                if expected_route.as_ref() != claim.progress().removal_target() {
                    return Err(RuntimeDrainClaimErrorV2::AcknowledgementProgressMismatch);
                }
                if claim
                    .progress()
                    .registry_observation_sequence()
                    .is_some_and(|refence_sequence| {
                        registry_observation_sequence <= refence_sequence
                    })
                {
                    return Err(
                        RuntimeDrainClaimErrorV2::AcknowledgementObservationNotAfterRefence,
                    );
                }
            }
        }
        Ok(Self {
            claim,
            expected_route,
            provenance,
            registry_observation_sequence,
            certification,
            acknowledged_at,
        })
    }

    pub fn claim(&self) -> &RuntimeDrainClaimV2 {
        &self.claim
    }

    pub fn expected_route(&self) -> Option<&RuntimeExactLocalRouteIdentityV2> {
        self.expected_route.as_ref()
    }

    pub fn provenance(&self) -> &RuntimeRouteMutationProvenanceV2 {
        &self.provenance
    }

    pub fn registry_observation_sequence(&self) -> NonZeroU64 {
        self.registry_observation_sequence
    }

    pub fn certification(&self) -> &RuntimeDrainCertificationResolutionV2 {
        &self.certification
    }

    pub fn acknowledged_at(&self) -> DateTime<Utc> {
        self.acknowledged_at
    }
}

pub(crate) fn validate_drain_claim_for_key(
    claim: &RuntimeDrainClaimV2,
    key: &RuntimeDrainIntentKeyV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_key(key)?;
    claim.validate_for_key(key)
}

pub(crate) fn validate_route_absent_acknowledgement_for_key(
    acknowledgement: &RuntimeRouteAbsentAcknowledgementV2,
    key: &RuntimeDrainIntentKeyV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_key(key)?;
    acknowledgement.claim.validate_for_key(key)?;
    acknowledgement
        .certification
        .validate_for(key, &acknowledgement.claim)
}

fn validate_key(key: &RuntimeDrainIntentKeyV2) -> Result<(), RuntimeDrainClaimErrorV2> {
    if !key.slot.matches_target(&key.expected_target) {
        return Err(RuntimeDrainClaimErrorV2::SlotMismatch);
    }
    validate_u64(
        key.expected_revision.get(),
        RuntimeDrainClaimFieldV2::ExpectedRevision,
    )?;
    validate_target(&key.expected_target)
}

fn validate_target(
    target: &automation_runtime_convergence::RuntimeDeploymentTargetV1,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    RuntimeDiscordSnowflakeV2::from_u64(target.guild_id.0)
        .map_err(|reason| canonical(RuntimeDrainClaimFieldV2::RouteGuildId, reason))?;
    validate_u64(
        target.binding_revision.get(),
        RuntimeDrainClaimFieldV2::RouteBindingRevision,
    )
}

fn validate_route(
    route: &RuntimeExactLocalRouteIdentityV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_target(&route.identity.target)?;
    validate_u64(
        route.identity.runtime_generation.get(),
        RuntimeDrainClaimFieldV2::RouteRuntimeGeneration,
    )?;
    validate_fencing_token(
        route.controller_fencing_token,
        RuntimeDrainClaimFieldV2::RouteControllerFencingToken,
    )?;
    validate_non_zero(
        route.route_incarnation,
        RuntimeDrainClaimFieldV2::RouteIncarnation,
    )
}

fn validate_owner_lease(
    owner: &RuntimeGatewayOwnerLeaseIdV1,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_non_zero(owner.lease_epoch, RuntimeDrainClaimFieldV2::OwnerLeaseEpoch)
}

fn validate_serving_identity(
    serving: &RuntimeServingIdentityV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_target_for_serving(&serving.process_identity)?;
    validate_non_zero(
        serving.lease_epoch,
        RuntimeDrainClaimFieldV2::ServingLeaseEpoch,
    )?;
    validate_non_zero(serving.revision, RuntimeDrainClaimFieldV2::ServingRevision)
}

fn validate_disconnected_revision(
    serving: &RuntimeServingIdentityV2,
    disconnected_revision: NonZeroU64,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    let expected = serving
        .revision
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeDrainClaimErrorV2::CertificationDisconnectRevisionMismatch)?;
    validate_non_zero(expected, RuntimeDrainClaimFieldV2::DisconnectedRevision)
        .map_err(|_| RuntimeDrainClaimErrorV2::CertificationDisconnectRevisionMismatch)?;
    if disconnected_revision == expected {
        Ok(())
    } else {
        Err(RuntimeDrainClaimErrorV2::CertificationDisconnectRevisionMismatch)
    }
}

fn validate_target_for_serving(
    identity: &RuntimeProcessIdentityV1,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    RuntimeDiscordSnowflakeV2::from_u64(identity.target.guild_id.0)
        .map_err(|reason| canonical(RuntimeDrainClaimFieldV2::RouteGuildId, reason))?;
    validate_u64(
        identity.target.binding_revision.get(),
        RuntimeDrainClaimFieldV2::ServingBindingRevision,
    )?;
    validate_u64(
        identity.runtime_generation.get(),
        RuntimeDrainClaimFieldV2::ServingRuntimeGeneration,
    )
}

fn validate_provenance(
    provenance: &RuntimeRouteMutationProvenanceV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    match provenance {
        RuntimeRouteMutationProvenanceV2::Ordinary { pause, .. } => {
            for value in [
                pause.coordinator_generation,
                pause.connection_epoch,
                pause.paused_admission_revision,
                pause.pause_sequence.into_non_zero(),
            ] {
                validate_non_zero(value, RuntimeDrainClaimFieldV2::ProvenanceInteger)?;
            }
        }
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => {
            validate_closed_recovery_provenance(witness)?;
        }
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => {
            validate_shutdown_provenance(witness)?;
        }
    }
    Ok(())
}

fn validate_closed_recovery_provenance(
    witness: &RuntimeClosedRecoveryRouteWitnessV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_owner_lease(&witness.gateway_owner_lease_id)?;
    for value in [
        witness.originating_emergency_generation,
        witness.recovery_generation,
        witness.recovery_authority_revision,
        witness.observed_owner_revision,
        witness.connection_epoch,
        witness.paused_admission_revision,
        witness.connected_event_sequence.into_non_zero(),
        witness.pause_sequence.into_non_zero(),
    ] {
        validate_non_zero(value, RuntimeDrainClaimFieldV2::ProvenanceInteger)?;
    }
    validate_datetime(
        witness.owner_expires_at,
        RuntimeDrainClaimFieldV2::ProvenanceOwnerExpiresAt,
    )?;
    if witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id {
        return Err(RuntimeDrainClaimErrorV2::OwnerMismatch);
    }
    Ok(())
}

fn validate_shutdown_provenance(
    witness: &RuntimeShutdownRouteWitnessV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_owner_lease(&witness.gateway_owner_lease_id)?;
    for value in [
        witness.shutdown_generation,
        witness.observed_owner_revision,
        witness.connection_epoch,
        witness.paused_admission_revision,
        witness.connected_event_sequence.into_non_zero(),
        witness.pause_sequence.into_non_zero(),
    ] {
        validate_non_zero(value, RuntimeDrainClaimFieldV2::ProvenanceInteger)?;
    }
    validate_datetime(
        witness.owner_expires_at,
        RuntimeDrainClaimFieldV2::ProvenanceOwnerExpiresAt,
    )?;
    if witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id {
        return Err(RuntimeDrainClaimErrorV2::OwnerMismatch);
    }
    Ok(())
}

fn validate_provenance_process(
    provenance: &RuntimeRouteMutationProvenanceV2,
    process_instance_id: &ProcessInstanceId,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    let provenance_process = match provenance {
        RuntimeRouteMutationProvenanceV2::Ordinary { .. } => return Ok(()),
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => &witness.process_instance_id,
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => &witness.process_instance_id,
    };
    if provenance_process == process_instance_id {
        Ok(())
    } else {
        Err(RuntimeDrainClaimErrorV2::ProvenanceMismatch)
    }
}

fn validate_provenance_owner(
    provenance: &RuntimeRouteMutationProvenanceV2,
    owner: &RuntimeGatewayOwnerLeaseIdV1,
    observed_owner_revision: NonZeroU64,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    let (provenance_owner, provenance_revision) = match provenance {
        RuntimeRouteMutationProvenanceV2::Ordinary { .. } => return Ok(()),
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => (
            &witness.gateway_owner_lease_id,
            witness.observed_owner_revision,
        ),
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => (
            &witness.gateway_owner_lease_id,
            witness.observed_owner_revision,
        ),
    };
    if provenance_owner != owner || provenance_revision != observed_owner_revision {
        Err(RuntimeDrainClaimErrorV2::OwnerMismatch)
    } else {
        Ok(())
    }
}

fn validate_non_zero(
    value: NonZeroU64,
    field: RuntimeDrainClaimFieldV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    RuntimePersistenceU64V2::from_non_zero(value)
        .map(|_| ())
        .map_err(|reason| canonical(field, reason))
}

fn validate_u64(
    value: u64,
    field: RuntimeDrainClaimFieldV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(|_| ())
        .map_err(|reason| canonical(field, reason))
}

fn validate_fencing_token(
    value: FencingToken,
    field: RuntimeDrainClaimFieldV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    validate_u64(value.get(), field)
}

fn validate_datetime(
    value: DateTime<Utc>,
    field: RuntimeDrainClaimFieldV2,
) -> Result<(), RuntimeDrainClaimErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(value)
        .map(|_| ())
        .map_err(|reason| canonical(field, reason))
}

fn canonical(
    field: RuntimeDrainClaimFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeDrainClaimErrorV2 {
    RuntimeDrainClaimErrorV2::CanonicalValue { field, reason }
}
