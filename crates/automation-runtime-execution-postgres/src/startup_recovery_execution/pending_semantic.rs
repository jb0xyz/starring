use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeCanonicalDrainIntentStateV2, RuntimeCanonicalProductDrainV2,
    RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2,
    RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2,
    RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2,
    RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2, RuntimeDrainClaimV2,
    RuntimeDrainIntentDigestV2, RuntimeDrainIntentV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimePersistedProductDrainRootV2, RuntimePersistedRouteAbsenceCandidateDrainIntentV2,
    RuntimePersistedUnclaimedPendingDrainIntentV2, RuntimeProductMutationDigestV2,
};
use automation_runtime_convergence::{
    ControllerId, FencingToken, RuntimeDeployment, RuntimeDeploymentSnapshotV1,
};
use automation_runtime_worker::{RuntimePendingDrainCandidateV2, RuntimePendingDrainStateDigestV2};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::closed_evidence::{
    closed_recovery_route_witness_v2, validate_closed_recovery_evidence_v2,
    RuntimeClosedRecoveryExpectedEvidenceV2, RuntimeClosedRecoveryProvenanceExpectationV2,
};
use super::pending_projection::{
    RuntimePendingDrainProductRootV2, RuntimePendingDrainProgressedProjectionV2,
};
use crate::RuntimeExecutionPersistenceErrorV1;

pub(super) struct RuntimePendingDrainExpectationV2<'a> {
    pub recovery_id: &'a str,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
    pub claim_action_authority_revision: i64,
    pub gateway_owner_lease_id: &'a RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: i64,
    pub owner_expires_at: DateTime<Utc>,
    pub candidate: RuntimePendingDrainCandidateV2,
    pub source_intent_revision: NonZeroU64,
    pub source_state_digest: RuntimePendingDrainStateDigestV2,
    pub prior_claim_terminal_digest: Option<&'a str>,
    pub seal: RuntimePendingDrainSealExpectationV2,
    pub evidence: &'a RuntimeClosedRecoveryExpectedEvidenceV2,
}

pub(super) struct RuntimePendingDrainSealExpectationV2 {
    pub pre_slot_admission_generation: Option<i64>,
    pub pre_slot_observation_sequence: Option<i64>,
    pub seal_generation: i64,
    pub post_admission_generation: i64,
    pub post_slot_observation_sequence: i64,
    pub post_global_sequence: i64,
    pub post_retained_slots: i64,
    pub post_retained_empty: i64,
    pub post_staged: i64,
    pub post_serving: i64,
    pub post_draining: i64,
    pub post_sealed: i64,
    pub post_active: i64,
    pub post_failed_closed_slots: i64,
}

pub(super) fn validate_pending_drain_claimed_projection_v2(
    projection: &RuntimePendingDrainProgressedProjectionV2,
    expected: &RuntimePendingDrainExpectationV2<'_>,
    minimum_database_now: DateTime<Utc>,
    database_now: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    validate_common(
        projection,
        expected,
        minimum_database_now,
        database_now,
        recorded_at,
    )?;
    if expected.claim_action_authority_revision != expected.action_authority_revision {
        return Err(invalid());
    }
    let root = persisted_root(&projection.product_root)?;
    let successor_revision = successor_revision(&projection.successor_state_bytes)?;
    let source_revision = predecessor(successor_revision)?;
    validate_stage_root(&projection.product_root, expected, source_revision, &root)?;
    let source_intent = RuntimeDrainIntentV2::pending_from_persisted(&root, source_revision, None)
        .map_err(|_| invalid())?;
    let source =
        RuntimeCanonicalDrainIntentStateV2::from_intent(source_intent).map_err(|_| invalid())?;
    validate_source_digest(&source, &projection.source_state_digest)?;
    let persisted_source = RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
        &root,
        source_revision,
        "pending",
        source.state_bytes(),
    )
    .map_err(|_| invalid())?;
    let expected_controller_id = expected_controller_id(expected)?;
    let expected_fence = successor_fence(projection.product_root.source_last_fencing_token)?;
    let transition = RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2::build(
        persisted_source,
        RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2 {
            recovery_witness: closed_recovery_route_witness_v2(&provenance_expectation(expected))?,
            controller_id: expected_controller_id.clone(),
            controller_fencing_token: FencingToken::new(positive_u64(expected_fence)?)
                .map_err(|_| invalid())?,
            claim_epoch: positive_non_zero(expected.coordinator_generation)?,
            claim_revision: NonZeroU64::MIN,
            claim_expires_at: expected.owner_expires_at,
            seal_generation: positive_non_zero(expected.seal.seal_generation)?,
            seal_observation_sequence: positive_non_zero(
                expected.seal.post_slot_observation_sequence,
            )?,
        },
    )
    .map_err(|_| invalid())?;
    let successor = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root,
        successor_revision,
        "pending",
        &projection.successor_state_bytes,
    )
    .map_err(|_| invalid())?;
    let claim = successor
        .intent()
        .state()
        .pending_claim()
        .ok_or_else(invalid)?;
    validate_claim(claim, expected, &expected_controller_id, expected_fence)?;
    if transition.result() != &successor {
        return Err(invalid());
    }
    validate_deployment_evidence(
        &projection.product_root,
        &root,
        RuntimePendingDrainDeploymentOutcomeV2::Claimed {
            controller_id: expected_controller_id.as_str(),
            fencing_token: expected_fence,
        },
    )
}

pub(super) fn validate_pending_drain_acknowledged_projection_v2(
    projection: &RuntimePendingDrainProgressedProjectionV2,
    expected: &RuntimePendingDrainExpectationV2<'_>,
    minimum_database_now: DateTime<Utc>,
    database_now: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    validate_common(
        projection,
        expected,
        minimum_database_now,
        database_now,
        recorded_at,
    )?;
    if expected
        .claim_action_authority_revision
        .checked_add(1)
        .is_none_or(|revision| revision != expected.action_authority_revision)
    {
        return Err(invalid());
    }
    let root = persisted_root(&projection.product_root)?;
    let successor_revision = successor_revision(&projection.successor_state_bytes)?;
    let source_revision = predecessor(successor_revision)?;
    validate_stage_root(&projection.product_root, expected, source_revision, &root)?;
    let successor = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root,
        successor_revision,
        "route_absent_acknowledged",
        &projection.successor_state_bytes,
    )
    .map_err(|_| invalid())?;
    let acknowledgement = successor
        .intent()
        .state()
        .acknowledgement()
        .ok_or_else(invalid)?;
    let source_intent = RuntimeDrainIntentV2::pending_from_persisted(
        &root,
        source_revision,
        Some(acknowledgement.claim().clone()),
    )
    .map_err(|_| invalid())?;
    let source =
        RuntimeCanonicalDrainIntentStateV2::from_intent(source_intent).map_err(|_| invalid())?;
    validate_source_digest(&source, &projection.source_state_digest)?;
    let persisted_source = RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
        &root,
        source_revision,
        "pending",
        source.state_bytes(),
    )
    .map_err(|_| invalid())?;
    let expected_controller_id = expected_controller_id(expected)?;
    let expected_fence = projection.product_root.source_last_fencing_token;
    validate_claim(
        acknowledgement.claim(),
        expected,
        &expected_controller_id,
        expected_fence,
    )?;
    if acknowledgement.acknowledged_at() != projection.product_root.transitioned_at
        || acknowledgement.registry_observation_sequence().get()
            != positive_u64(expected.seal.post_global_sequence)?
    {
        return Err(invalid());
    }
    let transition = RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2::build(
        persisted_source,
        RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2 {
            acknowledgement_observation_sequence: positive_non_zero(
                expected.seal.post_global_sequence,
            )?,
            certification: acknowledgement.certification().clone(),
            acknowledged_at: projection.product_root.transitioned_at,
            recovery_witness: closed_recovery_route_witness_v2(&provenance_expectation(expected))?,
        },
    )
    .map_err(|_| invalid())?;
    if transition.result() != &successor {
        return Err(invalid());
    }
    validate_deployment_evidence(
        &projection.product_root,
        &root,
        RuntimePendingDrainDeploymentOutcomeV2::Acknowledged {
            controller_id: acknowledgement.claim().controller_id().as_str(),
            fencing_token: acknowledgement.claim().controller_fencing_token().get(),
        },
    )
}

fn validate_common(
    projection: &RuntimePendingDrainProgressedProjectionV2,
    expected: &RuntimePendingDrainExpectationV2<'_>,
    minimum_database_now: DateTime<Utc>,
    database_now: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let transitioned_at = projection.product_root.transitioned_at;
    if expected
        .selection_authority_revision
        .checked_add(1)
        .is_none_or(|revision| revision != expected.action_authority_revision)
        || transitioned_at < minimum_database_now
        || transitioned_at > database_now
        || transitioned_at > recorded_at
        || transitioned_at >= expected.owner_expires_at
        || projection.source_state_digest
            != expected
                .source_state_digest
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
    {
        return Err(invalid());
    }
    validate_closed_recovery_evidence_v2(&projection.evidence, expected.evidence)
}

fn persisted_root(
    projection: &RuntimePendingDrainProductRootV2,
) -> Result<RuntimePersistedProductDrainRootV2, RuntimeExecutionPersistenceErrorV1> {
    let product_digest = RuntimeProductMutationDigestV2::parse(&projection.product_mutation_digest)
        .map_err(|_| invalid())?;
    let drain_digest = RuntimeDrainIntentDigestV2::parse(&projection.drain_intent_digest)
        .map_err(|_| invalid())?;
    let canonical = RuntimeCanonicalProductDrainV2::from_persisted(
        &projection.product_mutation_bytes,
        &product_digest,
        &projection.drain_intent_bytes,
        &drain_digest,
    )
    .map_err(|_| invalid())?;
    let product = canonical.product_preimage();
    let drain = &canonical.drain_preimage().key;
    let target = &product.expected_target;
    if projection.product_tenant_id != product.scope.tenant_id.as_str()
        || projection.product_installation_id != product.scope.installation_id.as_str()
        || projection.product_deployment_id != product.scope.deployment_id.as_str()
        || positive_u64(projection.product_expected_revision)? != product.expected_revision.get()
        || projection.product_operation_id != product.operation_id.as_str()
        || projection.drain_tenant_id != drain.scope.tenant_id.as_str()
        || projection.drain_installation_id != drain.scope.installation_id.as_str()
        || projection.drain_deployment_id != drain.scope.deployment_id.as_str()
        || projection.drain_slot_guild_id != drain.slot.guild_id.0.to_string()
        || projection.drain_slot_ruleset_key != drain.slot.ruleset_key.as_str()
        || positive_u64(projection.drain_expected_revision)? != drain.expected_revision.get()
        || projection.drain_intent_id != drain.intent_id.as_str()
        || projection.target_guild_id != target.guild_id.0.to_string()
        || projection.target_ruleset_key != target.ruleset_key.as_str()
        || positive_u64(projection.target_version)? != u64::from(target.version.get())
        || projection.target_content_hash != target.content_hash.to_hex()
        || positive_u64(projection.target_binding_revision)? != target.binding_revision.get()
        || projection.target_binding_fingerprint != target.binding_fingerprint.as_str()
        || drain.expected_target != *target
        || drain.slot != product.slot
    {
        return Err(invalid());
    }
    RuntimePersistedProductDrainRootV2::from_persisted(
        product.scope.clone(),
        product.expected_revision,
        &product.operation_id,
        drain.scope.clone(),
        drain.slot.clone(),
        drain.expected_revision,
        &drain.intent_id,
        &drain.expected_target,
        &projection.product_mutation_bytes,
        &product_digest,
        &projection.drain_intent_bytes,
        &drain_digest,
    )
    .map_err(|_| invalid())
}

enum RuntimePendingDrainDeploymentOutcomeV2<'a> {
    Claimed {
        controller_id: &'a str,
        fencing_token: i64,
    },
    Acknowledged {
        controller_id: &'a str,
        fencing_token: u64,
    },
}

fn validate_deployment_evidence(
    projection: &RuntimePendingDrainProductRootV2,
    root: &RuntimePersistedProductDrainRootV2,
    outcome: RuntimePendingDrainDeploymentOutcomeV2<'_>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let source_snapshot = decode_deployment_snapshot(&projection.source_deployment_snapshot_bytes)?;
    let successor_snapshot =
        decode_deployment_snapshot(&projection.successor_deployment_snapshot_bytes)?;
    let product = root.canonical().product_preimage();
    if !product.scope.matches(&source_snapshot.identity)
        || source_snapshot.target != product.expected_target
        || source_snapshot.revision.get() != positive_u64(projection.deployment_revision)?
        || source_snapshot.revision != product.expected_revision
        || source_snapshot.controller_lease.is_some()
        || successor_snapshot.controller_lease.is_some()
        || source_snapshot.identity != successor_snapshot.identity
        || source_snapshot.target != successor_snapshot.target
        || source_snapshot.runtime_generation != successor_snapshot.runtime_generation
        || source_snapshot.previous_runtime != successor_snapshot.previous_runtime
        || source_snapshot.requested_at != successor_snapshot.requested_at
        || source_snapshot.revision != successor_snapshot.revision
        || source_snapshot.phase != successor_snapshot.phase
        || deployment_phase_name(&source_snapshot)? != projection.deployment_phase
        || source_snapshot.last_fencing_token.map(FencingToken::get)
            != Some(positive_u64(projection.source_last_fencing_token)?)
        || successor_snapshot.last_fencing_token.map(FencingToken::get)
            != Some(positive_u64(projection.successor_last_fencing_token)?)
    {
        return Err(invalid());
    }
    let mut expected_successor = source_snapshot.clone();
    match outcome {
        RuntimePendingDrainDeploymentOutcomeV2::Claimed {
            controller_id,
            fencing_token,
        } => {
            if projection
                .source_last_fencing_token
                .checked_add(1)
                .is_none_or(|value| value != projection.successor_last_fencing_token)
                || projection.successor_last_controller_id != controller_id
                || projection.successor_last_fencing_token != fencing_token
            {
                return Err(invalid());
            }
            expected_successor.last_fencing_token =
                Some(FencingToken::new(positive_u64(fencing_token)?).map_err(|_| invalid())?);
        }
        RuntimePendingDrainDeploymentOutcomeV2::Acknowledged {
            controller_id,
            fencing_token,
        } => {
            if projection.successor_last_fencing_token != projection.source_last_fencing_token
                || projection.successor_last_controller_id != projection.source_last_controller_id
                || projection.source_last_controller_id != controller_id
                || positive_u64(projection.source_last_fencing_token)? != fencing_token
            {
                return Err(invalid());
            }
        }
    }
    if expected_successor != successor_snapshot {
        return Err(invalid());
    }
    Ok(())
}

fn decode_deployment_snapshot(
    bytes: &[u8],
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeExecutionPersistenceErrorV1> {
    let snapshot =
        serde_json::from_slice::<RuntimeDeploymentSnapshotV1>(bytes).map_err(|_| invalid())?;
    RuntimeDeployment::restore(snapshot.clone()).map_err(|_| invalid())?;
    Ok(snapshot)
}

fn deployment_phase_name(
    snapshot: &RuntimeDeploymentSnapshotV1,
) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
    serde_json::to_value(&snapshot.phase)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .and_then(|object| object.get("phase"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(invalid)
}

fn validate_claim(
    claim: &RuntimeDrainClaimV2,
    expected: &RuntimePendingDrainExpectationV2<'_>,
    expected_controller_id: &ControllerId,
    expected_fence: i64,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let seal = claim.progress().seal();
    if claim.gateway_owner_lease_id() != expected.gateway_owner_lease_id
        || claim.observed_owner_revision().get() != positive_u64(expected.owner_revision)?
        || claim.process_instance_id() != &expected.gateway_owner_lease_id.process_instance_id
        || claim.controller_id() != expected_controller_id
        || claim.controller_fencing_token().get() != positive_u64(expected_fence)?
        || claim.claim_epoch().get() != positive_u64(expected.coordinator_generation)?
        || claim.claim_revision() != NonZeroU64::MIN
        || claim.expires_at() != expected.owner_expires_at
        || seal.seal_generation().get() != positive_u64(expected.seal.seal_generation)?
        || seal.registry_observation_sequence().get()
            != positive_u64(expected.seal.post_slot_observation_sequence)?
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_stage_root(
    projection: &RuntimePendingDrainProductRootV2,
    expected: &RuntimePendingDrainExpectationV2<'_>,
    source_revision: NonZeroU64,
    root: &RuntimePersistedProductDrainRootV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let candidate = &expected.candidate;
    let target = candidate.expected_target();
    if positive_u64(projection.source_intent_revision)? != source_revision.get()
        || source_revision != expected.source_intent_revision
        || projection.drain_intent_id != candidate.intent_id().as_str()
        || projection.drain_slot_guild_id != candidate.slot().guild_id.to_string()
        || projection.drain_slot_ruleset_key != candidate.slot().ruleset_key.as_str()
        || projection.target_guild_id != target.guild_id.to_string()
        || projection.target_ruleset_key != target.ruleset_key.as_str()
        || positive_u64(projection.target_version)? != u64::from(target.version.get())
        || projection.target_content_hash != target.content_hash.to_hex()
        || positive_u64(projection.target_binding_revision)? != target.binding_revision.get()
        || projection.target_binding_fingerprint != target.binding_fingerprint.as_str()
        || projection.claim_action_authority_revision != expected.claim_action_authority_revision
        || projection.prior_claim_terminal_digest.as_deref() != expected.prior_claim_terminal_digest
    {
        return Err(invalid());
    }
    let seal = &projection.seal;
    let expected_seal = &expected.seal;
    let pre_slot = match (
        seal.pre_slot.as_ref(),
        expected_seal.pre_slot_admission_generation,
        expected_seal.pre_slot_observation_sequence,
    ) {
        (None, None, None) => None,
        (Some(actual), Some(admission), Some(observation))
            if actual.admission_generation == admission
                && actual.observation_sequence == observation =>
        {
            Some((admission, observation))
        }
        _ => return Err(invalid()),
    };
    let intent_key = root
        .canonical()
        .drain_preimage()
        .key
        .intent_id
        .canonical_bytes();
    if seal.seal_key != intent_key
        || seal.seal_generation != expected_seal.seal_generation
        || seal.post_admission_generation != expected_seal.post_admission_generation
        || seal.post_slot_observation_sequence != expected_seal.post_slot_observation_sequence
        || seal.pre_global_sequence != expected.evidence.registry_observation_sequence
        || seal.pre_retained_slots != expected.evidence.registry_retained_slot_count
        || seal.pre_retained_empty != expected.evidence.registry_retained_empty_tombstone_count
        || seal.post_global_sequence != expected_seal.post_global_sequence
        || seal.post_retained_slots != expected_seal.post_retained_slots
        || seal.post_retained_empty != expected_seal.post_retained_empty
        || seal.post_staged != expected_seal.post_staged
        || seal.post_serving != expected_seal.post_serving
        || seal.post_draining != expected_seal.post_draining
        || seal.post_sealed != expected_seal.post_sealed
        || seal.post_active != expected_seal.post_active
        || seal.post_failed_closed_slots != expected_seal.post_failed_closed_slots
        || seal
            .pre_global_sequence
            .checked_add(1)
            .is_none_or(|sequence| sequence != seal.post_global_sequence)
        || seal.pre_retained_slots != seal.pre_retained_empty
        || seal.post_staged != 0
        || seal.post_serving != 0
        || seal.post_draining != 0
        || seal.post_sealed != 1
        || seal.post_active != 0
        || seal.post_failed_closed_slots != 0
        || seal
            .post_retained_empty
            .checked_add(1)
            .is_none_or(|slots| slots != seal.post_retained_slots)
    {
        return Err(invalid());
    }
    match pre_slot {
        None => {
            if seal.post_admission_generation != 1
                || seal.post_slot_observation_sequence != 1
                || seal
                    .pre_retained_slots
                    .checked_add(1)
                    .is_none_or(|slots| slots != seal.post_retained_slots)
                || seal.post_retained_empty != seal.pre_retained_empty
            {
                return Err(invalid());
            }
        }
        Some((admission, observation)) => {
            if admission
                .checked_add(1)
                .is_none_or(|generation| generation != seal.post_admission_generation)
                || observation
                    .checked_add(1)
                    .is_none_or(|sequence| sequence != seal.post_slot_observation_sequence)
                || seal.post_retained_slots != seal.pre_retained_slots
                || seal
                    .post_retained_empty
                    .checked_add(1)
                    .is_none_or(|empty| empty != seal.pre_retained_empty)
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn expected_controller_id(
    expected: &RuntimePendingDrainExpectationV2<'_>,
) -> Result<ControllerId, RuntimeExecutionPersistenceErrorV1> {
    ControllerId::parse(format!(
        "recovery:{}:{}",
        expected.recovery_id, expected.claim_action_authority_revision
    ))
    .map_err(|_| invalid())
}

fn successor_revision(
    state_bytes: &[u8],
) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    let value = serde_json::from_slice::<serde_json::Value>(state_bytes).map_err(|_| invalid())?;
    let revision = value
        .as_object()
        .and_then(|root| root.get("intent_revision"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(invalid)?;
    if revision > i64::MAX as u64 {
        return Err(invalid());
    }
    NonZeroU64::new(revision).ok_or_else(invalid)
}

fn predecessor(successor: NonZeroU64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    successor
        .get()
        .checked_sub(1)
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid)
}

fn successor_fence(source: i64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    source
        .checked_add(1)
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}

fn validate_source_digest(
    source: &RuntimeCanonicalDrainIntentStateV2,
    expected_digest: &str,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let actual = Sha256::digest(source.state_bytes());
    let actual = actual
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual == expected_digest {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn provenance_expectation<'a>(
    expected: &'a RuntimePendingDrainExpectationV2<'a>,
) -> RuntimeClosedRecoveryProvenanceExpectationV2<'a> {
    RuntimeClosedRecoveryProvenanceExpectationV2 {
        recovery_id: expected.recovery_id,
        originating_emergency_generation: expected.originating_emergency_generation,
        coordinator_generation: expected.coordinator_generation,
        action_authority_revision: expected.action_authority_revision,
        gateway_owner_lease_id: expected.gateway_owner_lease_id,
        owner_revision: expected.owner_revision,
        owner_expires_at: expected.owner_expires_at,
        evidence: expected.evidence,
    }
}

fn positive_non_zero(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    NonZeroU64::new(positive_u64(value)?).ok_or_else(invalid)
}

fn positive_u64(value: i64) -> Result<u64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(invalid)
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use automation_runtime_controller::{
        GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeCertificationIntentFingerprintV2,
        RuntimeCertificationOperationIdV2, RuntimeDrainCertificationResolutionV2,
        RuntimeDrainIntentIdV2,
    };
    use automation_runtime_convergence::ProcessInstanceId;
    use serde_json::json;

    use super::*;
    use crate::startup_recovery_execution::closed_evidence::RuntimeClosedRecoveryEvidenceV2;
    use crate::startup_recovery_execution::pending_projection::RuntimePendingDrainSealBundleV2;

    const RECOVERY_ID: &str = "0123456789abcdef0123456789abcdef";
    const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
    const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";
    const PRODUCT_DIGEST: &str = "e35c1116d5bee2949184cceff540ee2575ac389461270f96f525ccd9c193166d";
    const DRAIN_DIGEST: &str = "edf1671e7c1395205cae7962d6cf043610c51b5ed49b2d4528d72351bed287fc";

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn product_bytes() -> Box<[u8]> {
        format!(
            concat!(
                "{{\"format_version\":2,\"operation_id\":\"{}\",",
                "\"scope\":{{\"tenant_id\":\"tenant:1\",\"installation_id\":",
                "\"installation:1\",\"deployment_id\":\"deployment:1\"}},",
                "\"expected_revision\":11,\"slot\":{{\"guild_id\":\"9223372036854775808\",",
                "\"ruleset_key\":\"studyroom\"}},\"expected_target\":{{\"guild_id\":",
                "\"9223372036854775808\",\"ruleset_key\":\"studyroom\",\"version\":1,",
                "\"content_hash\":\"{}\",\"binding_revision\":3,",
                "\"binding_fingerprint\":\"{}\"}},\"mutation_kind\":",
                "\"authority_change\",\"product_semantic_request_digest\":\"{}\"}}"
            ),
            PRODUCT_OPERATION_ID,
            "b".repeat(64),
            "a".repeat(64),
            "c".repeat(64),
        )
        .into_bytes()
        .into_boxed_slice()
    }

    fn drain_bytes() -> Box<[u8]> {
        format!(
            concat!(
                "{{\"format_version\":2,\"key\":{{\"intent_id\":\"{}\",",
                "\"product_operation_id\":\"{}\",",
                "\"product_mutation_digest\":\"{}\",",
                "\"scope\":{{\"tenant_id\":\"tenant:1\",\"installation_id\":",
                "\"installation:1\",\"deployment_id\":\"deployment:1\"}},",
                "\"expected_revision\":11,\"slot\":{{\"guild_id\":\"9223372036854775808\",",
                "\"ruleset_key\":\"studyroom\"}},\"expected_target\":{{\"guild_id\":",
                "\"9223372036854775808\",\"ruleset_key\":\"studyroom\",\"version\":1,",
                "\"content_hash\":\"{}\",\"binding_revision\":3,",
                "\"binding_fingerprint\":\"{}\"}},\"mutation_kind\":",
                "\"authority_change\"}}}}"
            ),
            DRAIN_INTENT_ID,
            PRODUCT_OPERATION_ID,
            PRODUCT_DIGEST,
            "b".repeat(64),
            "a".repeat(64),
        )
        .into_bytes()
        .into_boxed_slice()
    }

    fn snapshot(last_fencing_token: u64, phase: &str) -> Box<[u8]> {
        serde_json::to_vec(&json!({
            "identity": {
                "deployment_id": "deployment:1",
                "tenant_id": "tenant:1",
                "installation_id": "installation:1",
                "promotion_id": "9".repeat(64),
                "activation_request_id": "activation:1"
            },
            "target": {
                "guild_id": "9223372036854775808",
                "ruleset_key": "studyroom",
                "version": 1,
                "content_hash": "b".repeat(64),
                "binding_revision": 3,
                "binding_fingerprint": "a".repeat(64)
            },
            "runtime_generation": 9,
            "previous_runtime": null,
            "requested_at": at(1),
            "revision": 11,
            "phase": {
                "phase": phase
            },
            "controller_lease": null,
            "last_fencing_token": last_fencing_token,
            "preflight": null,
            "drain": null,
            "activation": null,
            "panel_certificate": null,
            "gateway_ready": null,
            "live": null,
            "last_live_recovery": null,
            "last_runtime_failure": null
        }))
        .unwrap()
        .into_boxed_slice()
    }

    fn product_root(claimed: bool) -> RuntimePendingDrainProductRootV2 {
        let source_fence = if claimed { 8 } else { 9 };
        let successor_fence = 9;
        let source_controller = if claimed {
            "controller:old"
        } else {
            "recovery:0123456789abcdef0123456789abcdef:5"
        };
        RuntimePendingDrainProductRootV2 {
            product_tenant_id: "tenant:1".to_owned(),
            product_installation_id: "installation:1".to_owned(),
            product_deployment_id: "deployment:1".to_owned(),
            product_expected_revision: 11,
            product_operation_id: PRODUCT_OPERATION_ID.to_owned(),
            drain_tenant_id: "tenant:1".to_owned(),
            drain_installation_id: "installation:1".to_owned(),
            drain_deployment_id: "deployment:1".to_owned(),
            drain_slot_guild_id: "9223372036854775808".to_owned(),
            drain_slot_ruleset_key: "studyroom".to_owned(),
            drain_expected_revision: 11,
            drain_intent_id: DRAIN_INTENT_ID.to_owned(),
            target_guild_id: "9223372036854775808".to_owned(),
            target_ruleset_key: "studyroom".to_owned(),
            target_version: 1,
            target_content_hash: "b".repeat(64),
            target_binding_revision: 3,
            target_binding_fingerprint: "a".repeat(64),
            product_mutation_bytes: product_bytes(),
            product_mutation_digest: PRODUCT_DIGEST.to_owned(),
            drain_intent_bytes: drain_bytes(),
            drain_intent_digest: DRAIN_DIGEST.to_owned(),
            source_intent_revision: if claimed { 1 } else { 2 },
            claim_action_authority_revision: 5,
            prior_claim_terminal_digest: if claimed { None } else { Some("e".repeat(64)) },
            seal: RuntimePendingDrainSealBundleV2 {
                pre_slot: None,
                seal_key: [
                    0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33,
                    0x22, 0x11, 0x00,
                ],
                seal_generation: 12,
                post_admission_generation: 1,
                post_slot_observation_sequence: 1,
                pre_global_sequence: 20,
                pre_retained_slots: 0,
                pre_retained_empty: 0,
                post_global_sequence: 21,
                post_retained_slots: 1,
                post_retained_empty: 0,
                post_staged: 0,
                post_serving: 0,
                post_draining: 0,
                post_sealed: 1,
                post_active: 0,
                post_failed_closed_slots: 0,
            },
            deployment_revision: 11,
            deployment_phase: "requested".to_owned(),
            source_last_fencing_token: source_fence,
            source_last_controller_id: source_controller.to_owned(),
            source_deployment_snapshot_bytes: snapshot(source_fence as u64, "requested"),
            successor_last_fencing_token: successor_fence,
            successor_last_controller_id: "recovery:0123456789abcdef0123456789abcdef:5".to_owned(),
            successor_deployment_snapshot_bytes: snapshot(successor_fence as u64, "requested"),
            transitioned_at: at(101),
        }
    }

    fn owner() -> RuntimeGatewayOwnerLeaseIdV1 {
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:gateway").unwrap(),
            lease_epoch: NonZeroU64::new(6).unwrap(),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        }
    }

    fn expected_evidence() -> RuntimeClosedRecoveryExpectedEvidenceV2 {
        RuntimeClosedRecoveryExpectedEvidenceV2 {
            paused_process_instance_id: "process:gateway".to_owned(),
            paused_coordinator_generation: 2,
            paused_connection_epoch: 4,
            paused_ready_kind: "ready",
            paused_admission_revision: 5,
            paused_transition_sequence: 8,
            paused_connected_event_sequence: 6,
            paused_last_resume_sequence: Some(7),
            registry_process_instance_id: "process:gateway".to_owned(),
            registry_observation_sequence: 20,
            registry_retained_slot_count: 0,
            registry_retained_empty_tombstone_count: 0,
        }
    }

    fn actual_evidence() -> RuntimeClosedRecoveryEvidenceV2 {
        RuntimeClosedRecoveryEvidenceV2 {
            paused_process_instance_id: "process:gateway".to_owned(),
            paused_coordinator_generation: 2,
            paused_connection_epoch: 4,
            paused_ready_kind: "ready",
            paused_admission_revision: 5,
            paused_transition_sequence: 8,
            paused_connected_event_sequence: 6,
            paused_last_resume_sequence: Some(7),
            registry_process_instance_id: "process:gateway".to_owned(),
            registry_observation_sequence: 20,
            registry_retained_slot_count: 0,
            registry_retained_empty_tombstone_count: 0,
        }
    }

    fn expectation<'a>(
        owner: &'a RuntimeGatewayOwnerLeaseIdV1,
        evidence: &'a RuntimeClosedRecoveryExpectedEvidenceV2,
    ) -> RuntimePendingDrainExpectationV2<'a> {
        let root = persisted_root(&product_root(true)).unwrap();
        let drain = &root.canonical().drain_preimage().key;
        let source = source(&root, 1, None);
        RuntimePendingDrainExpectationV2 {
            recovery_id: RECOVERY_ID,
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
            claim_action_authority_revision: 5,
            gateway_owner_lease_id: owner,
            owner_revision: 7,
            owner_expires_at: at(200),
            candidate: RuntimePendingDrainCandidateV2::new(
                drain.intent_id.clone(),
                drain.slot.clone(),
                drain.expected_target.clone(),
                NonZeroU64::MIN,
                RuntimePendingDrainStateDigestV2::new(Sha256::digest(source.state_bytes()).into())
                    .unwrap(),
            )
            .unwrap(),
            source_intent_revision: NonZeroU64::MIN,
            source_state_digest: RuntimePendingDrainStateDigestV2::new(
                Sha256::digest(source.state_bytes()).into(),
            )
            .unwrap(),
            prior_claim_terminal_digest: None,
            seal: RuntimePendingDrainSealExpectationV2 {
                pre_slot_admission_generation: None,
                pre_slot_observation_sequence: None,
                seal_generation: 12,
                post_admission_generation: 1,
                post_slot_observation_sequence: 1,
                post_global_sequence: 21,
                post_retained_slots: 1,
                post_retained_empty: 0,
                post_staged: 0,
                post_serving: 0,
                post_draining: 0,
                post_sealed: 1,
                post_active: 0,
                post_failed_closed_slots: 0,
            },
            evidence,
        }
    }

    fn acknowledgement_expectation<'a>(
        owner: &'a RuntimeGatewayOwnerLeaseIdV1,
        evidence: &'a RuntimeClosedRecoveryExpectedEvidenceV2,
    ) -> RuntimePendingDrainExpectationV2<'a> {
        let mut expected = expectation(owner, evidence);
        let claimed = claimed_projection();
        expected.selection_authority_revision = 5;
        expected.action_authority_revision = 6;
        expected.source_intent_revision = NonZeroU64::new(2).unwrap();
        expected.source_state_digest = RuntimePendingDrainStateDigestV2::new(
            Sha256::digest(&claimed.successor_state_bytes).into(),
        )
        .unwrap();
        expected.prior_claim_terminal_digest =
            Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        expected
    }

    fn source(
        root: &RuntimePersistedProductDrainRootV2,
        revision: u64,
        claim: Option<RuntimeDrainClaimV2>,
    ) -> RuntimeCanonicalDrainIntentStateV2 {
        RuntimeCanonicalDrainIntentStateV2::from_intent(
            RuntimeDrainIntentV2::pending_from_persisted(
                root,
                NonZeroU64::new(revision).unwrap(),
                claim,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn claimed_projection() -> RuntimePendingDrainProgressedProjectionV2 {
        let product_root = product_root(true);
        let root = persisted_root(&product_root).unwrap();
        let source = source(&root, 1, None);
        let owner = owner();
        let evidence = expected_evidence();
        let expected = expectation(&owner, &evidence);
        let persisted = RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
            &root,
            NonZeroU64::MIN,
            "pending",
            source.state_bytes(),
        )
        .unwrap();
        let transition = RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimTransitionV2::build(
            persisted,
            RuntimeClosedRecoveryEmptyRegistryPendingDrainClaimInputV2 {
                recovery_witness: closed_recovery_route_witness_v2(&provenance_expectation(
                    &expected,
                ))
                .unwrap(),
                controller_id: expected_controller_id(&expected).unwrap(),
                controller_fencing_token: FencingToken::new(9).unwrap(),
                claim_epoch: NonZeroU64::new(3).unwrap(),
                claim_revision: NonZeroU64::MIN,
                claim_expires_at: at(200),
                seal_generation: NonZeroU64::new(12).unwrap(),
                seal_observation_sequence: NonZeroU64::MIN,
            },
        )
        .unwrap();
        RuntimePendingDrainProgressedProjectionV2 {
            source_state_digest: lower_sha256(source.state_bytes()),
            successor_state_bytes: transition.result().state_bytes().into(),
            evidence: actual_evidence(),
            product_root,
        }
    }

    fn acknowledged_projection(
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> RuntimePendingDrainProgressedProjectionV2 {
        let claimed = claimed_projection();
        let root = persisted_root(&claimed.product_root).unwrap();
        let claimed_state = RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            NonZeroU64::new(2).unwrap(),
            "pending",
            &claimed.successor_state_bytes,
        )
        .unwrap();
        let persisted = RuntimePersistedRouteAbsenceCandidateDrainIntentV2::from_persisted(
            &root,
            NonZeroU64::new(2).unwrap(),
            "pending",
            claimed_state.state_bytes(),
        )
        .unwrap();
        let owner = owner();
        let evidence = expected_evidence();
        let mut expected = expectation(&owner, &evidence);
        expected.selection_authority_revision = 5;
        expected.action_authority_revision = 6;
        expected.prior_claim_terminal_digest =
            Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
        let transition = RuntimeClosedRecoveryPendingDrainAcknowledgementTransitionV2::build(
            persisted,
            RuntimeClosedRecoveryPendingDrainAcknowledgementInputV2 {
                acknowledgement_observation_sequence: NonZeroU64::new(21).unwrap(),
                certification,
                acknowledged_at: at(101),
                recovery_witness: closed_recovery_route_witness_v2(&provenance_expectation(
                    &expected,
                ))
                .unwrap(),
            },
        )
        .unwrap();
        RuntimePendingDrainProgressedProjectionV2 {
            source_state_digest: lower_sha256(claimed_state.state_bytes()),
            successor_state_bytes: transition.result().state_bytes().into(),
            evidence: actual_evidence(),
            product_root: product_root(false),
        }
    }

    fn lower_sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn assert_claimed_fails(projection: &RuntimePendingDrainProgressedProjectionV2) {
        let owner = owner();
        let evidence = expected_evidence();
        assert!(validate_pending_drain_claimed_projection_v2(
            projection,
            &expectation(&owner, &evidence),
            at(100),
            at(102),
            at(103),
        )
        .is_err());
    }

    fn assert_acknowledged_fails(projection: &RuntimePendingDrainProgressedProjectionV2) {
        let owner = owner();
        let evidence = expected_evidence();
        let expected = acknowledgement_expectation(&owner, &evidence);
        assert!(validate_pending_drain_acknowledged_projection_v2(
            projection,
            &expected,
            at(100),
            at(102),
            at(103),
        )
        .is_err());
    }

    #[test]
    fn claimed_projection_accepts_distinct_transition_action_and_record_clocks() {
        let projection = claimed_projection();
        let owner = owner();
        let evidence = expected_evidence();
        validate_pending_drain_claimed_projection_v2(
            &projection,
            &expectation(&owner, &evidence),
            at(100),
            at(102),
            at(103),
        )
        .unwrap();
    }

    #[test]
    fn transitioned_at_must_stay_inside_the_authorized_action_window() {
        let projection = claimed_projection();
        let owner = owner();
        let evidence = expected_evidence();
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &expectation(&owner, &evidence),
            at(102),
            at(103),
            at(104),
        )
        .is_err());
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &expectation(&owner, &evidence),
            at(100),
            at(100),
            at(103),
        )
        .is_err());
    }

    #[test]
    fn acknowledged_projection_uses_each_actual_certification_variant() {
        let owner = owner();
        let evidence = expected_evidence();
        let expected = acknowledgement_expectation(&owner, &evidence);
        for certification in [
            RuntimeDrainCertificationResolutionV2::no_operation_reserved(),
            RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
                RuntimeCertificationOperationIdV2::parse("11112222333344445555666677778888")
                    .unwrap(),
                RuntimeCertificationIntentFingerprintV2::parse("d".repeat(64)).unwrap(),
            ),
        ] {
            validate_pending_drain_acknowledged_projection_v2(
                &acknowledged_projection(certification),
                &expected,
                at(100),
                at(102),
                at(103),
            )
            .unwrap();
        }
    }

    #[test]
    fn normalized_identity_canonical_digest_and_source_digest_tampering_fail_closed() {
        let mut identity = claimed_projection();
        identity.product_root.product_installation_id = "installation:other".to_owned();
        assert_claimed_fails(&identity);

        let mut digest = claimed_projection();
        digest.product_root.product_mutation_digest = "0".repeat(64);
        assert_claimed_fails(&digest);

        let mut source_digest = claimed_projection();
        source_digest.source_state_digest = "0".repeat(64);
        assert_claimed_fails(&source_digest);

        let mut successor = claimed_projection();
        successor.successor_state_bytes[0] ^= 1;
        assert_claimed_fails(&successor);

        let projection = claimed_projection();
        let owner = owner();
        let evidence = expected_evidence();
        let mut candidate = expectation(&owner, &evidence);
        candidate.candidate = RuntimePendingDrainCandidateV2::new(
            RuntimeDrainIntentIdV2::parse("11112222333344445555666677778888").unwrap(),
            candidate.candidate.slot().clone(),
            candidate.candidate.expected_target().clone(),
            candidate.candidate.source_intent_revision(),
            candidate.candidate.source_state_digest().clone(),
        )
        .unwrap();
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &candidate,
            at(100),
            at(102),
            at(103),
        )
        .is_err());

        let mut expected_source = expectation(&owner, &evidence);
        expected_source.source_state_digest =
            RuntimePendingDrainStateDigestV2::new([9; 32]).unwrap();
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &expected_source,
            at(100),
            at(102),
            at(103),
        )
        .is_err());
    }

    #[test]
    fn claim_controller_fence_deployment_and_transition_time_tampering_fail_closed() {
        let mut source_fence = claimed_projection();
        source_fence.product_root.source_last_fencing_token = 7;
        assert_claimed_fails(&source_fence);

        let mut successor_fence = claimed_projection();
        successor_fence.product_root.successor_last_fencing_token = 10;
        assert_claimed_fails(&successor_fence);

        let mut controller = claimed_projection();
        controller.product_root.successor_last_controller_id = "controller:other".to_owned();
        assert_claimed_fails(&controller);

        let mut snapshot_tamper = claimed_projection();
        snapshot_tamper
            .product_root
            .successor_deployment_snapshot_bytes = snapshot(9, "cancelled");
        assert_claimed_fails(&snapshot_tamper);

        let mut transitioned_at = claimed_projection();
        transitioned_at.product_root.transitioned_at = at(104);
        assert_claimed_fails(&transitioned_at);
    }

    #[test]
    fn seal_and_closed_evidence_expectation_tampering_fail_closed() {
        let projection = claimed_projection();
        let owner = owner();
        let evidence = expected_evidence();
        let mut wrong_seal = expectation(&owner, &evidence);
        wrong_seal.seal.seal_generation = 13;
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &wrong_seal,
            at(100),
            at(102),
            at(103),
        )
        .is_err());

        let mut wrong_observation = expectation(&owner, &evidence);
        wrong_observation.seal.post_slot_observation_sequence = 2;
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &wrong_observation,
            at(100),
            at(102),
            at(103),
        )
        .is_err());

        let mut wrong_evidence = expected_evidence();
        wrong_evidence.registry_observation_sequence = 21;
        assert!(validate_pending_drain_claimed_projection_v2(
            &projection,
            &expectation(&owner, &wrong_evidence),
            at(100),
            at(102),
            at(103),
        )
        .is_err());
    }

    #[test]
    fn seal_key_stage_prior_digest_and_global_successor_tampering_fail_closed() {
        let mut seal_key = claimed_projection();
        seal_key.product_root.seal.seal_key[0] ^= 1;
        assert_claimed_fails(&seal_key);

        let mut source_revision = claimed_projection();
        source_revision.product_root.source_intent_revision = 2;
        assert_claimed_fails(&source_revision);

        let mut action = claimed_projection();
        action.product_root.claim_action_authority_revision = 6;
        assert_claimed_fails(&action);

        let mut global = claimed_projection();
        global.product_root.seal.post_global_sequence = 22;
        assert_claimed_fails(&global);

        let mut counts = claimed_projection();
        counts.product_root.seal.post_retained_slots = 2;
        assert_claimed_fails(&counts);

        let mut acknowledged =
            acknowledged_projection(RuntimeDrainCertificationResolutionV2::no_operation_reserved());
        acknowledged.product_root.prior_claim_terminal_digest = Some("f".repeat(64));
        assert_acknowledged_fails(&acknowledged);
    }

    #[test]
    fn acknowledged_deployment_and_certified_state_tampering_fail_closed() {
        let mut deployment =
            acknowledged_projection(RuntimeDrainCertificationResolutionV2::no_operation_reserved());
        deployment.product_root.successor_last_controller_id = "controller:other".to_owned();
        assert_acknowledged_fails(&deployment);

        let mut snapshot_tamper =
            acknowledged_projection(RuntimeDrainCertificationResolutionV2::no_operation_reserved());
        snapshot_tamper
            .product_root
            .successor_deployment_snapshot_bytes = snapshot(10, "requested");
        assert_acknowledged_fails(&snapshot_tamper);

        let mut successor =
            acknowledged_projection(RuntimeDrainCertificationResolutionV2::no_operation_reserved());
        let position = successor.successor_state_bytes.len() / 2;
        successor.successor_state_bytes[position] ^= 1;
        assert_acknowledged_fails(&successor);
    }
}
