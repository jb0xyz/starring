use std::num::NonZeroU64;

use automation_runtime_controller::{
    validate_compact_pending_drain_succession_v2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2,
    RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2,
    RuntimeCompactPendingDrainSuccessionValidationInputV2, RuntimeDrainCertificationResolutionV2,
};
use automation_runtime_convergence::ControllerId;
use automation_runtime_worker::{
    RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3, RuntimePendingDrainStateDigestV2,
};
use chrono::{DateTime, Utc};

use super::closed_evidence::{
    closed_recovery_route_witness_v2, RuntimeClosedRecoveryExpectedEvidenceV2,
    RuntimeClosedRecoveryProvenanceExpectationV2,
};
use super::digest::lowercase_sha256_bytes;
use super::pending::RuntimePendingDrainSealBindingsV2;
use super::pending_succession_projection::{
    decode_pending_drain_succession_terminal_projection_v3,
    RuntimePendingDrainSuccessionCertificationV3, RuntimePendingDrainSuccessionReadyKindV3,
};
use crate::RuntimeExecutionPersistenceErrorV1;

pub(super) struct RuntimePendingDrainSuccessionSemanticReceiptV3 {
    pub successor_intent_revision: NonZeroU64,
    pub successor_state_digest: RuntimePendingDrainStateDigestV2,
}

pub(super) fn validate_pending_drain_succession_projection_v3(
    projection_bytes: &[u8],
    authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    expected_evidence: &RuntimeClosedRecoveryExpectedEvidenceV2,
    expected_seal: &RuntimePendingDrainSealBindingsV2,
    minimum_database_now: DateTime<Utc>,
    database_now: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
) -> Result<RuntimePendingDrainSuccessionSemanticReceiptV3, RuntimeExecutionPersistenceErrorV1> {
    let projection = decode_pending_drain_succession_terminal_projection_v3(
        "route_absent_acknowledged",
        projection_bytes,
    )?;
    validate_evidence_v3(&projection, authorization, expected_evidence, expected_seal)?;
    let candidate = authorization.candidate();
    let request = authorization.request();
    let predecessor = candidate.predecessor_claim();
    validate_predecessor_v3(&projection, authorization)?;
    let transition_time =
        DateTime::from_timestamp_micros(projection.transition.database_now_unix_microseconds)
            .ok_or_else(invalid)?;
    if transition_time < minimum_database_now
        || transition_time < candidate.claim_expires_at()
        || transition_time > recorded_at
        || recorded_at > database_now
        || transition_time >= request.expected_owner_expires_at()
        || database_now >= request.expected_owner_expires_at()
    {
        return Err(invalid());
    }
    let certification = decode_certification_v3(&projection.transition.certification)?;
    let correlation = request.correlation();
    let action_authority_revision = positive_i64(correlation.authority_revision().get())?;
    let controller_id = ControllerId::parse(format!(
        "recovery:{}:{}",
        correlation.recovery_id().as_str(),
        action_authority_revision
    ))
    .map_err(|_| invalid())?;
    let recovery_witness =
        closed_recovery_route_witness_v2(&RuntimeClosedRecoveryProvenanceExpectationV2 {
            recovery_id: correlation.recovery_id().as_str(),
            originating_emergency_generation: positive_i64(
                correlation.originating_emergency_generation().get(),
            )?,
            coordinator_generation: positive_i64(correlation.coordinator_generation().get())?,
            action_authority_revision,
            gateway_owner_lease_id: request.gateway_owner_lease_id(),
            owner_revision: positive_i64(request.expected_owner_revision().get())?,
            owner_expires_at: request.expected_owner_expires_at(),
            evidence: expected_evidence,
        })?;
    let succession = RuntimeClosedRecoveryPendingDrainSuccessionAcknowledgementInputV2 {
        database_now: transition_time,
        recovery_witness,
        controller_id,
        seal_generation: positive_non_zero(expected_seal.seal_generation)?,
        seal_observation_sequence: positive_non_zero(expected_seal.post_slot_observation_sequence)?,
        acknowledgement_observation_sequence: positive_non_zero(
            expected_seal.post_global_observation_sequence,
        )?,
        certification,
        acknowledged_at: transition_time,
    };
    let predecessor_claim_source_digest =
        exact_digest(&projection.predecessor.predecessor_claim_source_digest)?;
    let validated = validate_compact_pending_drain_succession_v2(
        RuntimeCompactPendingDrainSuccessionValidationInputV2 {
            source_intent_revision: candidate.source_intent_revision(),
            source_state_digest: *candidate.source_state_digest().as_bytes(),
            predecessor_claim_source_digest,
            predecessor_claim: predecessor,
            succession: &succession,
            successor_state_bytes: &projection.successor_state_bytes,
        },
    )
    .map_err(|_| invalid())?;
    validate_transition_v3(&projection, authorization, &validated)?;
    Ok(RuntimePendingDrainSuccessionSemanticReceiptV3 {
        successor_intent_revision: validated.successor_intent_revision(),
        successor_state_digest: RuntimePendingDrainStateDigestV2::new(
            validated.successor_state_digest().to_owned(),
        )
        .map_err(|_| invalid())?,
    })
}

fn validate_evidence_v3(
    projection: &super::pending_succession_projection::RuntimePendingDrainSuccessionTerminalProjectionV3,
    authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    expected: &RuntimeClosedRecoveryExpectedEvidenceV2,
    seal: &RuntimePendingDrainSealBindingsV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let actual = &projection.closed_evidence;
    let correlation = authorization.request().correlation();
    let ready_kind = match actual.paused_ready_kind {
        RuntimePendingDrainSuccessionReadyKindV3::Ready => "ready",
        RuntimePendingDrainSuccessionReadyKindV3::Resumed => "resumed",
    };
    if actual.recovery_id != correlation.recovery_id().as_str()
        || actual.originating_emergency_generation
            != positive_i64(correlation.originating_emergency_generation().get())?
        || actual.coordinator_generation
            != positive_i64(correlation.coordinator_generation().get())?
        || actual.action_authority_revision != positive_i64(correlation.authority_revision().get())?
        || actual.selection_authority_revision
            != positive_i64(correlation.selection_authority_revision().get())?
        || actual.paused_process_instance_id != expected.paused_process_instance_id
        || actual.paused_coordinator_generation != expected.paused_coordinator_generation
        || actual.paused_connection_epoch != expected.paused_connection_epoch
        || ready_kind != expected.paused_ready_kind
        || actual.paused_admission_revision != expected.paused_admission_revision
        || actual.paused_transition_sequence != expected.paused_transition_sequence
        || actual.paused_connected_event_sequence != expected.paused_connected_event_sequence
        || actual.paused_last_resume_sequence != expected.paused_last_resume_sequence
        || actual.registry_process_instance_id != expected.registry_process_instance_id
        || actual.registry_observation_sequence != expected.registry_observation_sequence
        || actual.registry_retained_slot_count != expected.registry_retained_slot_count
        || actual.registry_retained_empty_tombstone_count
            != expected.registry_retained_empty_tombstone_count
    {
        return Err(invalid());
    }
    let actual_seal = &projection.seal_evidence;
    let pre_matches = match (actual_seal.pre_slot.as_ref(), seal.pre_slot_present) {
        (None, false) => true,
        (Some(actual), true) => {
            actual.admission_generation == seal.pre_slot_admission_generation
                && actual.observation_sequence == seal.pre_slot_observation_sequence
        }
        (None, true) | (Some(_), false) => false,
    };
    if !pre_matches
        || actual_seal.seal_key != seal.seal_key
        || actual_seal.seal_generation != seal.seal_generation
        || actual_seal.post_slot_admission_generation != seal.post_slot_admission_generation
        || actual_seal.post_slot_observation_sequence != seal.post_slot_observation_sequence
        || actual_seal.post_global_observation_sequence != seal.post_global_observation_sequence
        || actual_seal.post_global_retained_slot_count != seal.post_retained_slot_count
        || actual_seal.post_global_retained_empty_tombstone_count
            != seal.post_retained_empty_tombstone_count
        || actual_seal.post_global_staged_route_count != seal.post_staged_route_count
        || actual_seal.post_global_serving_route_count != seal.post_serving_route_count
        || actual_seal.post_global_draining_route_count != seal.post_draining_route_count
        || actual_seal.post_global_sealed_slot_count != seal.post_sealed_slot_count
        || actual_seal.post_global_active_interaction_count != seal.post_active_interaction_count
        || actual_seal.post_global_failed_closed_slot_count != seal.post_failed_closed_slot_count
        || actual_seal.post_global_registry_failed_closed != seal.post_registry_failed_closed
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_predecessor_v3(
    projection: &super::pending_succession_projection::RuntimePendingDrainSuccessionTerminalProjectionV3,
    authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let actual = &projection.predecessor;
    let candidate = authorization.candidate();
    let claim = candidate.predecessor_claim();
    let owner = claim.gateway_owner_lease_id();
    let seal = claim.progress().seal();
    if actual.drain_intent_id != candidate.intent_id().as_str()
        || positive_u64(actual.source_intent_revision)? != candidate.source_intent_revision().get()
        || exact_digest(&actual.source_state_digest)? != *candidate.source_state_digest().as_bytes()
        || exact_digest(&actual.predecessor_claim_terminal_digest)?
            != *candidate.predecessor_claim_terminal_digest().as_bytes()
        || actual.predecessor_gateway_shard_id != owner.gateway_shard_id.as_str()
        || actual.predecessor_process_instance_id != owner.process_instance_id.as_str()
        || actual.predecessor_process_instance_id != claim.process_instance_id().as_str()
        || positive_u64(actual.predecessor_lease_epoch)? != owner.lease_epoch.get()
        || actual.predecessor_runtime_build_revision != owner.expected_build_revision.as_str()
        || positive_u64(actual.predecessor_owner_revision)? != claim.observed_owner_revision().get()
        || actual.predecessor_controller_id != claim.controller_id().as_str()
        || positive_u64(actual.predecessor_controller_fencing_token)?
            != claim.controller_fencing_token().get()
        || positive_u64(actual.predecessor_claim_epoch)? != claim.claim_epoch().get()
        || positive_u64(actual.predecessor_claim_revision)? != claim.claim_revision().get()
        || actual.predecessor_claim_expires_at_unix_microseconds
            != claim.expires_at().timestamp_micros()
        || actual.predecessor_seal_process_instance_id != seal.process_instance_id().as_str()
        || positive_u64(actual.predecessor_seal_generation)? != seal.seal_generation().get()
        || positive_u64(actual.predecessor_seal_observation_sequence)?
            != seal.registry_observation_sequence().get()
    {
        return Err(invalid());
    }
    exact_digest(&actual.predecessor_claim_source_digest)?;
    Ok(())
}

fn validate_transition_v3(
    projection: &super::pending_succession_projection::RuntimePendingDrainSuccessionTerminalProjectionV3,
    authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    validated: &automation_runtime_controller::RuntimeValidatedCompactPendingDrainSuccessionV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let actual = &projection.transition;
    let key = validated.key();
    let target = &key.expected_target;
    let candidate = authorization.candidate();
    let predecessor = candidate.predecessor_claim();
    let expected_controller_id = format!(
        "recovery:{}:{}",
        authorization.request().correlation().recovery_id().as_str(),
        authorization
            .action_identity()
            .correlation()
            .authority_revision()
            .get()
    );
    let expected_successor_fence = predecessor
        .controller_fencing_token()
        .get()
        .checked_add(1)
        .ok_or_else(invalid)?;
    let expected_successor_claim_revision = predecessor
        .claim_revision()
        .get()
        .checked_add(1)
        .ok_or_else(invalid)?;
    if key.intent_id != *candidate.intent_id()
        || key.slot != *candidate.slot()
        || key.expected_target != *candidate.expected_target()
        || actual.tenant_id != key.scope.tenant_id.as_str()
        || actual.installation_id != key.scope.installation_id.as_str()
        || actual.deployment_id != key.scope.deployment_id.as_str()
        || positive_u64(actual.expected_revision)? != key.expected_revision.get()
        || actual.product_operation_id != key.product_operation_id.as_str()
        || actual.product_mutation_digest != key.product_mutation_digest.as_str()
        || exact_digest(&actual.product_mutation_request_digest)?
            != *candidate.product_mutation_request_sha256()
        || actual.drain_intent_digest != validated.drain_intent_digest().as_str()
        || exact_digest(&actual.drain_intent_request_digest)?
            != *candidate.drain_intent_request_sha256()
        || actual.slot_guild_id != key.slot.guild_id.to_string()
        || actual.slot_ruleset_key != key.slot.ruleset_key.as_str()
        || positive_u64(actual.target_version)? != u64::from(target.version.get())
        || actual.target_content_hash != target.content_hash.to_hex()
        || positive_u64(actual.target_binding_revision)? != target.binding_revision.get()
        || actual.target_binding_fingerprint != target.binding_fingerprint.as_str()
        || positive_u64(actual.source_fencing_token)?
            != predecessor.controller_fencing_token().get()
        || positive_u64(actual.successor_fencing_token)? != expected_successor_fence
        || actual.successor_controller_id != expected_controller_id
        || positive_u64(actual.successor_claim_revision)? != expected_successor_claim_revision
        || positive_u64(actual.successor_intent_revision)?
            != validated.successor_intent_revision().get()
        || exact_digest(&actual.successor_state_digest)? != *validated.successor_state_digest()
        || &decode_certification_v3(&actual.certification)? != validated.certification()
    {
        return Err(invalid());
    }
    Ok(())
}

fn decode_certification_v3(
    certification: &RuntimePendingDrainSuccessionCertificationV3,
) -> Result<RuntimeDrainCertificationResolutionV2, RuntimeExecutionPersistenceErrorV1> {
    match certification {
        RuntimePendingDrainSuccessionCertificationV3::NoOperationReserved {} => {
            Ok(RuntimeDrainCertificationResolutionV2::no_operation_reserved())
        }
        RuntimePendingDrainSuccessionCertificationV3::NoAttestationForReservedOperation {
            operation_id,
            intent_fingerprint,
        } => Ok(
            RuntimeDrainCertificationResolutionV2::no_attestation_for_reserved_operation(
                RuntimeCertificationOperationIdV2::parse(operation_id.clone())
                    .map_err(|_| invalid())?,
                RuntimeCertificationIntentFingerprintV2::parse(intent_fingerprint.clone())
                    .map_err(|_| invalid())?,
            ),
        ),
        RuntimePendingDrainSuccessionCertificationV3::CommittedAndDisconnected { .. } => {
            Err(invalid())
        }
    }
}

fn exact_digest(value: &str) -> Result<[u8; 32], RuntimeExecutionPersistenceErrorV1> {
    lowercase_sha256_bytes(value)
}

fn positive_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}

fn positive_u64(value: i64) -> Result<u64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}

fn positive_non_zero(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    positive_u64(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid)
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}
