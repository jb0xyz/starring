use std::fmt::{Debug, Formatter, Write};
use std::future::Future;
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeCanonicalDrainIntentStateV2, RuntimeCanonicalDrainIntentStateV3,
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeDrainActionDigestV3,
    RuntimeDrainCanonicalStateDigestV3, RuntimeDrainCertificationResolutionV2,
    RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimV2, RuntimeDrainIntentCanonicalStateKindV2,
    RuntimeDrainIntentCanonicalStateKindV3, RuntimeDrainIntentIdV2,
    RuntimeExactLocalRouteIdentityV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimePersistedRefencedPendingDrainIntentV2,
    RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    RuntimePersistedUnclaimedPendingDrainIntentV2,
    RuntimePreviousProcessDrainCertificationResolutionV3, RuntimePreviousProcessDrainProgressV3,
    RuntimeRouteMutationProvenanceV2, RuntimeServingIdentityV2, RuntimeServingReceiptV2,
    RuntimeServingSlotV2, RuntimeUnixMicrosecondsV2,
};
use automation_runtime_convergence::{
    FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryExecutionActionIdentityV2, RuntimeStartupRecoveryExecutionCorrelationV2,
    RuntimeStartupRecoveryExecutionTerminalDigestV2,
};

mod orchestration;
#[cfg(test)]
mod tests;

pub use orchestration::{
    RuntimeAcknowledgedPendingDrainV4, RuntimeAuthorizedRegistryRefenceEvidenceV4,
    RuntimeAuthorizedRegistryRefenceV4, RuntimeDrainRefenceProgressExecutionPortV4,
    RuntimeDrainRefenceProgressExecutionResolutionV4, RuntimeDrainRefenceProgressPortOutcomeV4,
    RuntimeDurablePreviousProcessTeardownBoundaryV4, RuntimeDurableRefenceBoundaryV4,
    RuntimeDurableRefencePortObservationV4, RuntimeDurableRoutedClaimBoundaryV4,
    RuntimeDurableSameProcessAcknowledgementBoundaryV4, RuntimeDurablyRefencedBoundaryV4,
    RuntimeEmptySuccessionPortObservationV4, RuntimeLocalRefencePortObservationV4,
    RuntimeLocalRefenceProgressV4, RuntimePendingDrainBoundaryErrorV4,
    RuntimePendingDrainCertificationResolutionPortV4, RuntimePendingDrainLaneJoinedV4,
    RuntimePendingDrainMutationPortReceiptV4, RuntimePendingDrainRegistryTransitionPortV4,
    RuntimePendingDrainServingLanePortV4, RuntimePendingDrainServingObservationPortV4,
    RuntimePendingDrainServingResolvedV4, RuntimePreparedPreviousProcessTeardownV4,
    RuntimePreviousProcessDrainTeardownExecutionPortV4,
    RuntimePreviousProcessDrainTeardownExecutionResolutionV4,
    RuntimePreviousProcessDrainTeardownPortOutcomeV4, RuntimePreviousProcessTeardownEvidencePortV4,
    RuntimePreviousProcessTeardownRegistrationV4, RuntimePreviousProcessTeardownV4,
    RuntimeRecoveredRouteAbsentRegistrationV4, RuntimeRouteAbsentAcknowledgementV4,
    RuntimeRouteAbsentPortObservationV4, RuntimeRoutedClaimedContinuationV4,
    RuntimeRoutedClaimedSealPortObservationV4, RuntimeRoutedDrainClaimExecutionPortV4,
    RuntimeRoutedDrainClaimExecutionResolutionV4, RuntimeRoutedDrainClaimPortOutcomeV4,
    RuntimeRoutedDrainDeterminateNonCommitPortObservationV4,
    RuntimeRoutedDrainRollbackAuthorizationV4, RuntimeRoutedDrainRollbackPortV4,
    RuntimeRoutedSealPortObservationV4, RuntimeRoutedSealedClaimV4,
    RuntimeSameProcessDrainAcknowledgementExecutionPortV4,
    RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4,
    RuntimeSameProcessDrainAcknowledgementPortOutcomeV4,
    RuntimeSuccessionAcknowledgedPendingDrainV4,
};

mod mutation;
mod selection;
mod terminal;

pub use mutation::*;
use mutation::{
    RuntimeAcknowledgementAuthorizationSourceV4, RuntimeAppliedRoutedClaimSourceV4,
    RuntimePreviousProcessTeardownSourceV4, RuntimeRefenceAuthorizationSourceV4,
};
use selection::RuntimePendingDrainCandidateCommonV4;
pub use selection::*;
pub use terminal::*;

pub(crate) fn authorize_pending_drain_selection_v4(
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
) -> Result<RuntimeAuthorizedPendingDrainSelectionV4, RuntimePendingDrainV4Error> {
    if authorization.request().class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
        return Err(RuntimePendingDrainV4Error::ActionClassMismatch);
    }
    authorization
        .request()
        .action_identity()
        .correlation()
        .authority_revision()
        .get()
        .checked_add(3)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(RuntimePendingDrainV4Error::AuthorityRevisionOverflow)?;
    Ok(RuntimeAuthorizedPendingDrainSelectionV4 { authorization })
}

fn expected_teardown_provenance(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    action_identity: &RuntimePendingDrainActionIdentityV4,
) -> Result<RuntimeRouteMutationProvenanceV2, RuntimePendingDrainV4Error> {
    let correlation = action_identity.correlation();
    let paused = request.paused_gateway();
    if paused.process_instance_id() != request.registry_process_instance_id()
        || paused.process_instance_id() != &request.gateway_owner_lease_id().process_instance_id
        || paused.coordinator_generation().get()
            != correlation.originating_emergency_generation().get()
    {
        return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
    }
    Ok(RuntimeRouteMutationProvenanceV2::ClosedRecovery(
        RuntimeClosedRecoveryRouteWitnessV2 {
            recovery_id: correlation.recovery_id().clone(),
            originating_emergency_generation: correlation.originating_emergency_generation(),
            recovery_generation: correlation.coordinator_generation(),
            recovery_authority_revision: correlation.authority_revision(),
            gateway_owner_lease_id: request.gateway_owner_lease_id().clone(),
            observed_owner_revision: request.expected_owner_revision(),
            owner_expires_at: request.expected_owner_expires_at(),
            process_instance_id: paused.process_instance_id().clone(),
            connection_epoch: paused.connection_epoch(),
            paused_admission_revision: paused.admission_revision(),
            connected_event_sequence: paused.connected_event_sequence(),
            pause_sequence: paused.transition_sequence(),
        },
    ))
}

fn terminal_digest_v3(
    digest: &RuntimeStartupRecoveryExecutionTerminalDigestV2,
) -> Result<RuntimeDrainActionDigestV3, RuntimePendingDrainV4Error> {
    let mut encoded = String::with_capacity(64);
    for byte in digest.as_bytes() {
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| RuntimePendingDrainV4Error::MutationReceiptMismatch)?;
    }
    RuntimeDrainActionDigestV3::parse(encoded)
        .map_err(|_| RuntimePendingDrainV4Error::MutationReceiptMismatch)
}

fn validate_claim_fence(
    common: &RuntimePendingDrainCandidateCommonV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    let claim = common
        .claim()
        .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
    if claim.controller_fencing_token() != common.source_deployment_fence {
        return Err(RuntimePendingDrainV4Error::ControllerFenceMismatch);
    }
    validate_fence(claim.controller_fencing_token())
}

fn validate_candidate_request(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    common: &RuntimePendingDrainCandidateCommonV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    validate_request_owner(request, &common.current_owner)?;
    if request.registry_process_instance_id() != &common.current_owner.lease_id.process_instance_id
    {
        return Err(RuntimePendingDrainV4Error::RegistryProcessMismatch);
    }
    Ok(())
}

fn validate_request_owner(
    request: &crate::RuntimeStartupRecoveryExecutionRequestV2,
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<(), RuntimePendingDrainV4Error> {
    validate_owner_current(owner)?;
    if request.gateway_owner_lease_id() != &owner.lease_id
        || request.expected_owner_revision() != owner.owner_revision
        || request.expected_owner_expires_at() != owner.expires_at
        || owner.database_now < request.minimum_database_now()
    {
        return Err(RuntimePendingDrainV4Error::OwnerMismatch);
    }
    Ok(())
}

fn validate_owner_current(
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<(), RuntimePendingDrainV4Error> {
    validate_database_time(owner.database_now)?;
    validate_database_time(owner.expires_at)?;
    validate_persistence_value(owner.owner_revision)?;
    validate_persistence_value(owner.lease_id.lease_epoch)?;
    if owner.database_now >= owner.expires_at {
        return Err(RuntimePendingDrainV4Error::OwnerExpired);
    }
    Ok(())
}

fn validate_current_claim(
    claim: &RuntimeDrainClaimV2,
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<(), RuntimePendingDrainV4Error> {
    validate_database_time(claim.expires_at())?;
    if claim.gateway_owner_lease_id() != &owner.lease_id
        || claim.observed_owner_revision() != owner.owner_revision
        || claim.process_instance_id() != &owner.lease_id.process_instance_id
    {
        return Err(RuntimePendingDrainV4Error::CurrentOwnerClaimMismatch);
    }
    Ok(())
}

fn validate_previous_claim(
    claim: &RuntimeDrainClaimV2,
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    expect_expired: bool,
) -> Result<(), RuntimePendingDrainV4Error> {
    validate_database_time(claim.expires_at())?;
    let predecessor = claim.gateway_owner_lease_id();
    if predecessor.process_instance_id == owner.lease_id.process_instance_id {
        return Err(RuntimePendingDrainV4Error::StableOwnerProcessMismatch);
    }
    if predecessor.gateway_shard_id != owner.lease_id.gateway_shard_id {
        return Err(RuntimePendingDrainV4Error::OwnerShardMismatch);
    }
    if predecessor.lease_epoch >= owner.lease_id.lease_epoch {
        return Err(RuntimePendingDrainV4Error::OwnerEpochNotNewer);
    }
    let expired = owner.database_now >= claim.expires_at();
    if expired != expect_expired {
        return Err(RuntimePendingDrainV4Error::ClaimExpiryClassificationMismatch);
    }
    Ok(())
}

fn previous_retry(
    claim: &RuntimeDrainClaimV2,
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<Duration, RuntimePendingDrainV4Error> {
    positive_duration(claim.expires_at(), owner.database_now)
        .zip(positive_duration(owner.expires_at, owner.database_now))
        .map(|(claim, owner)| Duration::from_secs(1).min(claim).min(owner))
        .ok_or(RuntimePendingDrainV4Error::ClaimExpiryClassificationMismatch)
}

fn validate_serving(
    evidence: &RuntimePendingDrainServingEvidenceV4,
    scope: automation_runtime_controller::RuntimeDeploymentScopeV1,
    target: &RuntimeDeploymentTargetV1,
    claim: Option<&RuntimeDrainClaimV2>,
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<(), RuntimePendingDrainV4Error> {
    let (receipt, database_now) = match evidence {
        RuntimePendingDrainServingEvidenceV4::Absent { observed_at, .. } => {
            validate_database_time(*observed_at)?;
            if *observed_at != owner.database_now {
                return Err(RuntimePendingDrainV4Error::ServingEvidenceMismatch);
            }
            return Ok(());
        }
        RuntimePendingDrainServingEvidenceV4::Observed {
            receipt,
            database_now,
            ..
        } => (receipt, database_now),
    };
    validate_database_time(*database_now)?;
    validate_database_time(receipt.acquired_at)?;
    validate_database_time(receipt.last_heartbeat_at)?;
    validate_database_time(receipt.expires_at)?;
    validate_persistence_value(receipt.identity.lease_epoch)?;
    validate_persistence_value(receipt.identity.revision)?;
    if *database_now != owner.database_now
        || receipt.identity.scope != scope
        || receipt.identity.process_identity.target != *target
        || receipt.last_heartbeat_at < receipt.acquired_at
        || receipt.expires_at < receipt.last_heartbeat_at
    {
        return Err(RuntimePendingDrainV4Error::ServingEvidenceMismatch);
    }
    let expected_process = claim
        .map(RuntimeDrainClaimV2::process_instance_id)
        .unwrap_or(&owner.lease_id.process_instance_id);
    if &receipt.identity.process_identity.process_instance_id != expected_process {
        return Err(RuntimePendingDrainV4Error::ServingEvidenceMismatch);
    }
    Ok(())
}

fn validate_certification(
    evidence: &RuntimePendingDrainCertificationEvidenceV4,
    serving: &RuntimePendingDrainServingEvidenceV4,
    scope: automation_runtime_controller::RuntimeDeploymentScopeV1,
    target: &RuntimeDeploymentTargetV1,
    claim: Option<&RuntimeDrainClaimV2>,
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
) -> Result<(), RuntimePendingDrainV4Error> {
    let expected_process = claim
        .map(RuntimeDrainClaimV2::process_instance_id)
        .unwrap_or(&owner.lease_id.process_instance_id);
    match evidence {
        RuntimePendingDrainCertificationEvidenceV4::NoOperationReserved { .. } => {}
        RuntimePendingDrainCertificationEvidenceV4::NoAttestationForReservedOperation {
            operation_id,
            ..
        } => {
            if let RuntimePendingDrainServingEvidenceV4::Observed { receipt, .. } = serving {
                if receipt.identity.operation_id != *operation_id {
                    return Err(RuntimePendingDrainV4Error::CertificationEvidenceMismatch);
                }
            }
        }
        RuntimePendingDrainCertificationEvidenceV4::Committed {
            serving_identity, ..
        } => {
            if serving_identity.scope != scope
                || serving_identity.process_identity.target != *target
                || &serving_identity.process_identity.process_instance_id != expected_process
            {
                return Err(RuntimePendingDrainV4Error::CertificationEvidenceMismatch);
            }
            if let RuntimePendingDrainServingEvidenceV4::Observed { receipt, .. } = serving {
                if receipt.identity != **serving_identity {
                    return Err(RuntimePendingDrainV4Error::CertificationEvidenceMismatch);
                }
            }
        }
    }
    Ok(())
}

fn validate_resolution(
    common: &RuntimePendingDrainCandidateCommonV4,
    resolution: &RuntimeDrainCertificationResolutionV2,
) -> Result<(), RuntimePendingDrainV4Error> {
    match &common.certification {
        RuntimePendingDrainCertificationEvidenceV4::NoOperationReserved { .. } => {
            if resolution.kind()
                != automation_runtime_controller::RuntimeDrainCertificationResolutionKindV2::NoOperationReserved
            {
                return Err(RuntimePendingDrainV4Error::CertificationResolutionMismatch);
            }
        }
        RuntimePendingDrainCertificationEvidenceV4::NoAttestationForReservedOperation {
            operation_id,
            intent_fingerprint,
            ..
        } => {
            if resolution.kind()
                != automation_runtime_controller::RuntimeDrainCertificationResolutionKindV2::NoAttestationForReservedOperation
                || resolution.operation_id() != Some(operation_id)
                || resolution.intent_fingerprint() != Some(intent_fingerprint)
            {
                return Err(RuntimePendingDrainV4Error::CertificationResolutionMismatch);
            }
        }
        RuntimePendingDrainCertificationEvidenceV4::Committed {
            serving_identity, ..
        } => {
            if resolution.kind()
                != automation_runtime_controller::RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
                || resolution.serving_identity() != Some(serving_identity)
            {
                return Err(RuntimePendingDrainV4Error::CertificationResolutionMismatch);
            }
        }
    }
    Ok(())
}

fn validate_resolved_serving(
    common: &RuntimePendingDrainCandidateCommonV4,
    resolution: &RuntimeDrainCertificationResolutionV2,
) -> Result<(), RuntimePendingDrainV4Error> {
    if resolution.kind()
        != automation_runtime_controller::RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
    {
        return Ok(());
    }
    match &common.serving {
        RuntimePendingDrainServingEvidenceV4::Absent { .. } => Ok(()),
        RuntimePendingDrainServingEvidenceV4::Observed {
            receipt,
            database_now,
            ..
        } => {
            if resolution.serving_identity() != Some(&receipt.identity)
                || (receipt.connected && receipt.expires_at > *database_now)
            {
                Err(RuntimePendingDrainV4Error::ServingEvidenceMismatch)
            } else {
                Ok(())
            }
        }
    }
}

fn validate_routed_seal(
    common: &RuntimePendingDrainCandidateCommonV4,
    seal: &RuntimeRoutedSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    if common.intent_id() != &seal.intent_id
        || common.slot() != &seal.slot
        || common.expected_target() != &seal.route.identity.target
        || common.current_owner.lease_id.process_instance_id != seal.process_instance_id
        || common.source_deployment_fence != seal.route.controller_fencing_token
        || seal.seal_key != common.intent_id().canonical_bytes()
    {
        return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
    }
    Ok(())
}

fn validate_routed_claimed_seal(
    common: &RuntimePendingDrainCandidateCommonV4,
    seal: &RuntimeRoutedClaimedSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    validate_routed_seal(common, &seal.routed_seal)?;
    let claim = common
        .claim()
        .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
    let journal = common
        .claim_journal
        .as_ref()
        .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?;
    if seal.claim_fence != claim.controller_fencing_token()
        || seal.claim_receipt_digest.as_bytes() != journal.terminal_digest.as_bytes()
    {
        return Err(RuntimePendingDrainV4Error::RegistryReceiptMismatch);
    }
    Ok(())
}

fn validate_durable_refence_witness(
    common: &RuntimePendingDrainCandidateCommonV4,
    durable: &RuntimeDurablyRefencedSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    let claim = common
        .claim()
        .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
    let claim_journal = common
        .claim_journal
        .as_ref()
        .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?;
    let refence_journal = common
        .refence_journal
        .as_ref()
        .ok_or(RuntimePendingDrainV4Error::RefenceJournalMissing)?;
    let local = &durable.locally_refenced;
    if common.intent_id() != &local.claimed.routed_seal.intent_id
        || common.slot() != &local.claimed.routed_seal.slot
        || common.current_owner.lease_id.process_instance_id
            != local.claimed.routed_seal.process_instance_id
        || claim.progress().old_route() != Some(&local.old_route)
        || claim.progress().removal_target() != Some(&local.removal_target)
        || claim.progress().provenance() != Some(&local.provenance)
        || claim.progress().registry_observation_sequence()
            != Some(local.registry_observation_sequence)
        || local.claimed.claim_receipt_digest.as_bytes() != claim_journal.terminal_digest.as_bytes()
        || durable.refence_receipt_digest.as_bytes() != refence_journal.terminal_digest.as_bytes()
    {
        return Err(RuntimePendingDrainV4Error::RegistryReceiptMismatch);
    }
    Ok(())
}

fn validate_route_absent(
    common: &RuntimePendingDrainCandidateCommonV4,
    route_absent: &RuntimeRouteAbsentSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    let claim = common
        .claim()
        .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
    let claim_journal = common
        .claim_journal
        .as_ref()
        .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?;
    let refence_journal = common
        .refence_journal
        .as_ref()
        .ok_or(RuntimePendingDrainV4Error::RefenceJournalMissing)?;
    validate_route_absent_for_claim(common, claim, route_absent)?;
    if route_absent.claim_receipt_digest.as_bytes() != claim_journal.terminal_digest.as_bytes()
        || route_absent.refence_receipt_digest.as_bytes()
            != refence_journal.terminal_digest.as_bytes()
    {
        return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
    }
    Ok(())
}

fn validate_route_absent_for_claim(
    common: &RuntimePendingDrainCandidateCommonV4,
    claim: &RuntimeDrainClaimV2,
    route_absent: &RuntimeRouteAbsentSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    if common.intent_id() != &route_absent.intent_id
        || common.slot() != &route_absent.slot
        || common.current_owner.lease_id.process_instance_id != route_absent.process_instance_id
        || route_absent.seal_key != common.intent_id().canonical_bytes()
        || claim.progress().old_route() != Some(&route_absent.source_route)
        || claim.progress().removal_target() != Some(&route_absent.removed_route)
    {
        return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
    }
    Ok(())
}

fn validate_empty_succession_seal(
    common: &RuntimePendingDrainCandidateCommonV4,
    seal: &RuntimeEmptySuccessionSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    let claim = common
        .claim()
        .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
    let predecessor_route = claim
        .progress()
        .seal()
        .expected_route()
        .ok_or(RuntimePendingDrainV4Error::RouteMissing)?;
    if common.intent_id() != &seal.intent_id
        || common.slot() != &seal.slot
        || common.current_owner.lease_id.process_instance_id != seal.process_instance_id
        || seal.seal_key != common.intent_id().canonical_bytes()
        || predecessor_route != &seal.predecessor_route
        || seal.possible_route_fence_ceiling != claim.controller_fencing_token()
        || seal.successor_fence
            != claim
                .controller_fencing_token()
                .next()
                .map_err(|_| RuntimePendingDrainV4Error::ControllerFenceOverflow)?
    {
        return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
    }
    Ok(())
}

fn validate_terminal_receipt(
    identity: &RuntimePendingDrainTerminalIdentityV4,
    receipt: &RuntimePendingDrainMutationReceiptV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    if receipt.action_identity != identity.action_identity
        || receipt.source_intent_revision != identity.source_intent_revision
        || receipt.source_state_digest != identity.source_state_digest
    {
        return Err(RuntimePendingDrainV4Error::TerminalIdentityMismatch);
    }
    Ok(())
}

fn validate_typed_v2_result(
    result: &RuntimeCanonicalDrainIntentStateV2,
    mutation: &RuntimePendingDrainMutationReceiptV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    if result.intent().intent_revision() != mutation.result_intent_revision
        || result.state_bytes() != mutation.result_state_bytes.as_ref()
        || RuntimeDrainCanonicalStateDigestV3::from_state_bytes(result.state_bytes())
            != mutation.result_state_digest
    {
        return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
    }
    Ok(())
}

fn validate_mutation_source(
    action_identity: &RuntimePendingDrainActionIdentityV4,
    common: &RuntimePendingDrainCandidateCommonV4,
    mutation: &RuntimePendingDrainMutationReceiptV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    if &mutation.action_identity != action_identity
        || mutation.source_intent_revision != common.canonical().intent().intent_revision()
        || mutation.source_state_digest != common.source_state_digest
        || mutation.owner_receipt != common.current_owner
        || mutation.committed_at < common.selection_database_now
    {
        return Err(RuntimePendingDrainV4Error::MutationReceiptMismatch);
    }
    Ok(())
}

fn validate_registry_values(
    seal_generation: NonZeroU64,
    admission_generation: NonZeroU64,
    slot_observation_sequence: NonZeroU64,
    registry_observation_sequence: NonZeroU64,
) -> Result<(), RuntimePendingDrainV4Error> {
    for value in [
        seal_generation,
        admission_generation,
        slot_observation_sequence,
        registry_observation_sequence,
    ] {
        validate_persistence_value(value)?;
    }
    Ok(())
}

fn validate_successor_budget(
    value: NonZeroU64,
    successors: u64,
) -> Result<(), RuntimePendingDrainV4Error> {
    value
        .get()
        .checked_add(successors)
        .filter(|next| *next <= i64::MAX as u64)
        .ok_or(RuntimePendingDrainV4Error::IntentRevisionOverflow)?;
    Ok(())
}

fn validate_persistence_value(value: NonZeroU64) -> Result<(), RuntimePendingDrainV4Error> {
    if value.get() > i64::MAX as u64 {
        return Err(RuntimePendingDrainV4Error::PersistenceValueOutOfRange);
    }
    Ok(())
}

fn validate_database_time(value: DateTime<Utc>) -> Result<(), RuntimePendingDrainV4Error> {
    RuntimeUnixMicrosecondsV2::from_datetime(value)
        .map(|_| ())
        .map_err(|_| RuntimePendingDrainV4Error::DatabaseTimeOutOfRange)
}

fn validate_fence(value: FencingToken) -> Result<(), RuntimePendingDrainV4Error> {
    if value.get() > i64::MAX as u64 {
        return Err(RuntimePendingDrainV4Error::PersistenceValueOutOfRange);
    }
    Ok(())
}

fn positive_duration(later: DateTime<Utc>, earlier: DateTime<Utc>) -> Option<Duration> {
    later
        .signed_duration_since(earlier)
        .to_std()
        .ok()
        .filter(|duration| !duration.is_zero())
}

fn provenance_process(provenance: &RuntimeRouteMutationProvenanceV2) -> Option<&ProcessInstanceId> {
    match provenance {
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) => {
            Some(&witness.process_instance_id)
        }
        RuntimeRouteMutationProvenanceV2::Shutdown(witness) => Some(&witness.process_instance_id),
        RuntimeRouteMutationProvenanceV2::Ordinary { .. } => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePendingDrainV4Error {
    #[error("runtime pending drain V4 digest is zero")]
    ZeroDigest,
    #[error("runtime pending drain V4 source canonical digest does not match")]
    SourceDigestMismatch,
    #[error("runtime pending drain V4 database clock does not match owner observation")]
    DatabaseClockMismatch,
    #[error("runtime pending drain V4 database clock regressed")]
    DatabaseClockRegressed,
    #[error("runtime pending drain V4 database time is outside the canonical range")]
    DatabaseTimeOutOfRange,
    #[error("runtime pending drain V4 owner is expired")]
    OwnerExpired,
    #[error("runtime pending drain V4 owner does not match recovery authority")]
    OwnerMismatch,
    #[error("runtime pending drain V4 current-owner claim does not match")]
    CurrentOwnerClaimMismatch,
    #[error("runtime pending drain V4 stable owner process is not distinct")]
    StableOwnerProcessMismatch,
    #[error("runtime pending drain V4 owner shard does not match")]
    OwnerShardMismatch,
    #[error("runtime pending drain V4 successor owner epoch is not newer")]
    OwnerEpochNotNewer,
    #[error("runtime pending drain V4 claim expiry class does not match database time")]
    ClaimExpiryClassificationMismatch,
    #[error("runtime pending drain V4 action class does not match")]
    ActionClassMismatch,
    #[error("runtime pending drain V4 action correlation does not match")]
    CorrelationMismatch,
    #[error("runtime pending drain V4 authority revision overflow")]
    AuthorityRevisionOverflow,
    #[error("runtime pending drain V4 intent revision overflow")]
    IntentRevisionOverflow,
    #[error("runtime pending drain V4 controller fence overflow")]
    ControllerFenceOverflow,
    #[error("runtime pending drain V4 persistence value is out of range")]
    PersistenceValueOutOfRange,
    #[error("runtime pending drain V4 claim is missing")]
    ClaimMissing,
    #[error("runtime pending drain V4 route is missing")]
    RouteMissing,
    #[error("runtime pending drain V4 progress does not match")]
    ProgressMismatch,
    #[error("runtime pending drain V4 route lineage does not match")]
    RouteLineageMismatch,
    #[error("runtime pending drain V4 controller fence does not match")]
    ControllerFenceMismatch,
    #[error("runtime pending drain V4 claim action journal is missing")]
    ClaimJournalMissing,
    #[error("runtime pending drain V4 refence action journal is missing")]
    RefenceJournalMissing,
    #[error("runtime pending drain V4 action journal is unexpected")]
    UnexpectedJournal,
    #[error("runtime pending drain V4 action journal revision is not an exact successor")]
    JournalRevisionMismatch,
    #[error("runtime pending drain V4 action journal process does not match")]
    JournalProcessMismatch,
    #[error("runtime pending drain V4 claim action journal does not match")]
    ClaimJournalMismatch,
    #[error("runtime pending drain V4 refence action journal does not match")]
    RefenceJournalMismatch,
    #[error("runtime pending drain V4 serving evidence does not match")]
    ServingEvidenceMismatch,
    #[error("runtime pending drain V4 certification evidence does not match")]
    CertificationEvidenceMismatch,
    #[error("runtime pending drain V4 certification resolution does not match")]
    CertificationResolutionMismatch,
    #[error("runtime pending drain V4 registry process does not match")]
    RegistryProcessMismatch,
    #[error("runtime pending drain V4 registry witness does not match")]
    RegistryWitnessMismatch,
    #[error("runtime pending drain V4 registry durable receipt does not match")]
    RegistryReceiptMismatch,
    #[error("runtime pending drain V4 registry still has active guards")]
    ActiveGuards,
    #[error("runtime pending drain V4 serving lease is fresh for {0:?}")]
    ServingLeaseFresh(Duration),
    #[error("runtime pending drain V4 route-absent state requires the existing V2 or V3 handoff")]
    LegacyRouteAbsentHandoffRequired,
    #[error("runtime pending drain V4 mutation receipt does not match")]
    MutationReceiptMismatch,
    #[error("runtime pending drain V4 terminal identity does not match")]
    TerminalIdentityMismatch,
    #[error("runtime pending drain V4 finalizer identity does not match")]
    FinalizerIdentityMismatch,
    #[error("runtime pending drain V4 determinate non-commit evidence does not match")]
    DeterminateNonCommitMismatch,
}
