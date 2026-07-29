use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::RuntimeExecutionPersistenceErrorV1;

const TERMINAL_PROJECTION_DOMAIN: &[u8] =
    b"starring.runtime.startup_recovery.pending_drain.succession.terminal.v3";
const TERMINAL_PROJECTION_VERSION: i16 = 3;
const TERMINAL_PROJECTION_TAG: i16 = 3;
const MAX_TERMINAL_PROJECTION_BYTES: usize = 131_072;
const MAX_PREDECESSOR_FRAME_BYTES: usize = 8_192;
const MAX_SUCCESSOR_STATE_FRAME_BYTES: usize = 1_048_576;
const MAX_EVIDENCE_FRAME_BYTES: usize = 16_384;
const MAX_TRANSITION_FRAME_BYTES: usize = 16_384;
const ABSENT_TAG: i16 = 0;
const PRESENT_TAG: i16 = 1;
const READY_TAG: i16 = 1;
const RESUMED_TAG: i16 = 2;
const SEAL_BUNDLE_VERSION: i16 = 3;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainSuccessionTerminalProjectionV3 {
    pub predecessor: RuntimePendingDrainSuccessionPredecessorV3,
    pub successor_state_bytes: Box<[u8]>,
    pub closed_evidence: RuntimePendingDrainSuccessionClosedEvidenceV3,
    pub seal_evidence: RuntimePendingDrainSuccessionSealEvidenceV3,
    pub transition: RuntimePendingDrainSuccessionTransitionV3,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePendingDrainSuccessionPredecessorV3 {
    pub drain_intent_id: String,
    pub source_intent_revision: i64,
    pub source_state_digest: String,
    pub predecessor_claim_terminal_digest: String,
    pub predecessor_gateway_shard_id: String,
    pub predecessor_process_instance_id: String,
    pub predecessor_lease_epoch: i64,
    pub predecessor_runtime_build_revision: String,
    pub predecessor_owner_revision: i64,
    pub predecessor_controller_id: String,
    pub predecessor_controller_fencing_token: i64,
    pub predecessor_claim_epoch: i64,
    pub predecessor_claim_revision: i64,
    pub predecessor_claim_expires_at_unix_microseconds: i64,
    pub predecessor_seal_process_instance_id: String,
    pub predecessor_seal_generation: i64,
    pub predecessor_seal_observation_sequence: i64,
    pub predecessor_claim_source_digest: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainSuccessionClosedEvidenceV3 {
    pub recovery_id: String,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
    pub paused_process_instance_id: String,
    pub paused_coordinator_generation: i64,
    pub paused_connection_epoch: i64,
    pub paused_ready_kind: RuntimePendingDrainSuccessionReadyKindV3,
    pub paused_admission_revision: i64,
    pub paused_transition_sequence: i64,
    pub paused_connected_event_sequence: i64,
    pub paused_last_resume_sequence: Option<i64>,
    pub registry_process_instance_id: String,
    pub registry_observation_sequence: i64,
    pub registry_retained_slot_count: i64,
    pub registry_retained_empty_tombstone_count: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimePendingDrainSuccessionReadyKindV3 {
    Ready,
    Resumed,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainSuccessionSealEvidenceV3 {
    pub pre_slot: Option<RuntimePendingDrainSuccessionPreSlotEvidenceV3>,
    pub seal_key: [u8; 16],
    pub seal_generation: i64,
    pub post_slot_admission_generation: i64,
    pub post_slot_observation_sequence: i64,
    pub post_global_observation_sequence: i64,
    pub post_global_retained_slot_count: i64,
    pub post_global_retained_empty_tombstone_count: i64,
    pub post_global_staged_route_count: i64,
    pub post_global_serving_route_count: i64,
    pub post_global_draining_route_count: i64,
    pub post_global_sealed_slot_count: i64,
    pub post_global_active_interaction_count: i64,
    pub post_global_failed_closed_slot_count: i64,
    pub post_global_registry_failed_closed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainSuccessionPreSlotEvidenceV3 {
    pub admission_generation: i64,
    pub observation_sequence: i64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePendingDrainSuccessionTransitionV3 {
    pub tenant_id: String,
    pub installation_id: String,
    pub deployment_id: String,
    pub expected_revision: i64,
    pub product_operation_id: String,
    pub product_mutation_digest: String,
    pub product_mutation_request_digest: String,
    pub drain_intent_digest: String,
    pub drain_intent_request_digest: String,
    pub slot_guild_id: String,
    pub slot_ruleset_key: String,
    pub target_version: i64,
    pub target_content_hash: String,
    pub target_binding_revision: i64,
    pub target_binding_fingerprint: String,
    pub source_fencing_token: i64,
    pub successor_fencing_token: i64,
    pub successor_controller_id: String,
    pub successor_claim_revision: i64,
    pub successor_intent_revision: i64,
    pub successor_state_digest: String,
    pub certification: RuntimePendingDrainSuccessionCertificationV3,
    pub database_now_unix_microseconds: i64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum RuntimePendingDrainSuccessionCertificationV3 {
    #[serde(rename = "no_operation_reserved")]
    NoOperationReserved {},
    #[serde(rename = "no_attestation_for_reserved_operation")]
    NoAttestationForReservedOperation {
        operation_id: String,
        intent_fingerprint: String,
    },
    #[serde(rename = "committed_and_disconnected")]
    CommittedAndDisconnected {
        operation_id: String,
        serving_identity: Box<RuntimePendingDrainSuccessionServingIdentityV3>,
        disconnected_revision: u64,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePendingDrainSuccessionServingIdentityV3 {
    pub scope: RuntimePendingDrainSuccessionDeploymentScopeV3,
    pub operation_id: String,
    pub attestation_digest: String,
    pub process_identity: RuntimePendingDrainSuccessionProcessIdentityV3,
    pub lease_epoch: u64,
    pub revision: u64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePendingDrainSuccessionDeploymentScopeV3 {
    pub tenant_id: String,
    pub installation_id: String,
    pub deployment_id: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePendingDrainSuccessionProcessIdentityV3 {
    pub target: RuntimePendingDrainSuccessionDeploymentTargetV3,
    pub runtime_generation: u64,
    pub process_instance_id: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePendingDrainSuccessionDeploymentTargetV3 {
    pub guild_id: String,
    pub ruleset_key: String,
    pub version: u32,
    pub content_hash: String,
    pub binding_revision: u64,
    pub binding_fingerprint: String,
}

pub(super) fn decode_pending_drain_succession_terminal_projection_v3(
    terminal_outcome_name: &str,
    projection: &[u8],
) -> Result<RuntimePendingDrainSuccessionTerminalProjectionV3, RuntimeExecutionPersistenceErrorV1> {
    if terminal_outcome_name != "route_absent_acknowledged"
        || projection.is_empty()
        || projection.len() > MAX_TERMINAL_PROJECTION_BYTES
    {
        return Err(invalid());
    }
    let mut projection_cursor = Cursor::new(projection);
    let domain_length = usize::try_from(projection_cursor.take_i64()?).map_err(|_| invalid())?;
    if domain_length != TERMINAL_PROJECTION_DOMAIN.len()
        || projection_cursor.take(domain_length)? != TERMINAL_PROJECTION_DOMAIN
        || projection_cursor.take_i16()? != TERMINAL_PROJECTION_VERSION
        || projection_cursor.take_i16()? != TERMINAL_PROJECTION_TAG
    {
        return Err(invalid());
    }
    let remainder = projection_cursor.remainder();
    let digest_start = remainder.len().checked_sub(32).ok_or_else(invalid)?;
    let (framed_payload, persisted_digest) = remainder.split_at(digest_start);
    if Sha256::digest(framed_payload).as_slice() != persisted_digest {
        return Err(invalid());
    }
    let mut frame_cursor = Cursor::new(framed_payload);
    let predecessor_frame = frame_cursor.take_nonempty_frame(MAX_PREDECESSOR_FRAME_BYTES)?;
    let successor_state_frame =
        frame_cursor.take_nonempty_frame(MAX_SUCCESSOR_STATE_FRAME_BYTES)?;
    let evidence_frame = frame_cursor.take_nonempty_frame(MAX_EVIDENCE_FRAME_BYTES)?;
    let transition_frame = frame_cursor.take_nonempty_frame(MAX_TRANSITION_FRAME_BYTES)?;
    if !frame_cursor.is_empty() {
        return Err(invalid());
    }
    let predecessor = serde_json::from_slice(predecessor_frame).map_err(|_| invalid())?;
    let transition = serde_json::from_slice(transition_frame).map_err(|_| invalid())?;
    let (closed_evidence, seal_evidence) = decode_evidence_frame(evidence_frame)?;
    Ok(RuntimePendingDrainSuccessionTerminalProjectionV3 {
        predecessor,
        successor_state_bytes: successor_state_frame.to_vec().into_boxed_slice(),
        closed_evidence,
        seal_evidence,
        transition,
    })
}

fn decode_evidence_frame(
    frame: &[u8],
) -> Result<
    (
        RuntimePendingDrainSuccessionClosedEvidenceV3,
        RuntimePendingDrainSuccessionSealEvidenceV3,
    ),
    RuntimeExecutionPersistenceErrorV1,
> {
    let mut cursor = Cursor::new(frame);
    let recovery_id_bytes = cursor.take_fixed::<32>()?;
    if !recovery_id_bytes
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(invalid());
    }
    let recovery_id = std::str::from_utf8(&recovery_id_bytes)
        .map(str::to_owned)
        .map_err(|_| invalid())?;
    let originating_emergency_generation = cursor.take_positive_i64()?;
    let coordinator_generation = cursor.take_positive_i64()?;
    let action_authority_revision = cursor.take_positive_i64()?;
    let selection_authority_revision = cursor.take_positive_i64()?;
    let paused_process_instance_id = cursor.take_nonempty_text_frame()?;
    let paused_coordinator_generation = cursor.take_positive_i64()?;
    let paused_connection_epoch = cursor.take_positive_i64()?;
    let paused_ready_kind = match cursor.take_i16()? {
        READY_TAG => RuntimePendingDrainSuccessionReadyKindV3::Ready,
        RESUMED_TAG => RuntimePendingDrainSuccessionReadyKindV3::Resumed,
        _ => return Err(invalid()),
    };
    let paused_admission_revision = cursor.take_positive_i64()?;
    let paused_transition_sequence = cursor.take_positive_i64()?;
    let paused_connected_event_sequence = cursor.take_positive_i64()?;
    let paused_last_resume_sequence = match cursor.take_i16()? {
        ABSENT_TAG => None,
        PRESENT_TAG => Some(cursor.take_positive_i64()?),
        _ => return Err(invalid()),
    };
    let registry_process_instance_id = cursor.take_nonempty_text_frame()?;
    let registry_observation_sequence = cursor.take_positive_i64()?;
    let registry_retained_slot_count = cursor.take_nonnegative_i64()?;
    let registry_retained_empty_tombstone_count = cursor.take_nonnegative_i64()?;
    let seal_frame = cursor.take_nonempty_frame(MAX_EVIDENCE_FRAME_BYTES)?;
    if !cursor.is_empty()
        || paused_transition_sequence <= paused_connected_event_sequence
        || paused_last_resume_sequence.is_some_and(|sequence| {
            sequence <= paused_connected_event_sequence || sequence > paused_transition_sequence
        })
        || registry_retained_slot_count != registry_retained_empty_tombstone_count
    {
        return Err(invalid());
    }
    let closed_evidence = RuntimePendingDrainSuccessionClosedEvidenceV3 {
        recovery_id,
        originating_emergency_generation,
        coordinator_generation,
        action_authority_revision,
        selection_authority_revision,
        paused_process_instance_id,
        paused_coordinator_generation,
        paused_connection_epoch,
        paused_ready_kind,
        paused_admission_revision,
        paused_transition_sequence,
        paused_connected_event_sequence,
        paused_last_resume_sequence,
        registry_process_instance_id,
        registry_observation_sequence,
        registry_retained_slot_count,
        registry_retained_empty_tombstone_count,
    };
    Ok((closed_evidence, decode_seal_frame(seal_frame)?))
}

fn decode_seal_frame(
    frame: &[u8],
) -> Result<RuntimePendingDrainSuccessionSealEvidenceV3, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    if cursor.take_i16()? != SEAL_BUNDLE_VERSION {
        return Err(invalid());
    }
    let pre_slot = match cursor.take_i16()? {
        ABSENT_TAG => None,
        PRESENT_TAG => Some(RuntimePendingDrainSuccessionPreSlotEvidenceV3 {
            admission_generation: cursor.take_positive_i64()?,
            observation_sequence: cursor.take_positive_i64()?,
        }),
        _ => return Err(invalid()),
    };
    let seal = RuntimePendingDrainSuccessionSealEvidenceV3 {
        pre_slot,
        seal_key: cursor.take_fixed::<16>()?,
        seal_generation: cursor.take_positive_i64()?,
        post_slot_admission_generation: cursor.take_positive_i64()?,
        post_slot_observation_sequence: cursor.take_positive_i64()?,
        post_global_observation_sequence: cursor.take_positive_i64()?,
        post_global_retained_slot_count: cursor.take_nonnegative_i64()?,
        post_global_retained_empty_tombstone_count: cursor.take_nonnegative_i64()?,
        post_global_staged_route_count: cursor.take_nonnegative_i64()?,
        post_global_serving_route_count: cursor.take_nonnegative_i64()?,
        post_global_draining_route_count: cursor.take_nonnegative_i64()?,
        post_global_sealed_slot_count: cursor.take_nonnegative_i64()?,
        post_global_active_interaction_count: cursor.take_nonnegative_i64()?,
        post_global_failed_closed_slot_count: cursor.take_nonnegative_i64()?,
        post_global_registry_failed_closed: match cursor.take_i16()? {
            ABSENT_TAG => false,
            PRESENT_TAG => true,
            _ => return Err(invalid()),
        },
    };
    if !cursor.is_empty() {
        return Err(invalid());
    }
    Ok(seal)
}

struct Cursor<'a> {
    remainder: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(value: &'a [u8]) -> Self {
        Self { remainder: value }
    }

    fn take_i16(&mut self) -> Result<i16, RuntimeExecutionPersistenceErrorV1> {
        Ok(i16::from_be_bytes(self.take_fixed::<2>()?))
    }

    fn take_i64(&mut self) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
        Ok(i64::from_be_bytes(self.take_fixed::<8>()?))
    }

    fn take_positive_i64(&mut self) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
        let value = self.take_i64()?;
        if value <= 0 {
            Err(invalid())
        } else {
            Ok(value)
        }
    }

    fn take_nonnegative_i64(&mut self) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
        let value = self.take_i64()?;
        if value < 0 {
            Err(invalid())
        } else {
            Ok(value)
        }
    }

    fn take_nonempty_frame(
        &mut self,
        maximum: usize,
    ) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let length = usize::try_from(self.take_i64()?).map_err(|_| invalid())?;
        if length == 0 || length > maximum {
            return Err(invalid());
        }
        self.take(length)
    }

    fn take_nonempty_text_frame(&mut self) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
        std::str::from_utf8(self.take_nonempty_frame(MAX_EVIDENCE_FRAME_BYTES)?)
            .map(str::to_owned)
            .map_err(|_| invalid())
    }

    fn take_fixed<const LENGTH: usize>(
        &mut self,
    ) -> Result<[u8; LENGTH], RuntimeExecutionPersistenceErrorV1> {
        self.take(LENGTH)?.try_into().map_err(|_| invalid())
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let (value, remainder) = self
            .remainder
            .split_at_checked(length)
            .ok_or_else(invalid)?;
        self.remainder = remainder;
        Ok(value)
    }

    fn remainder(&self) -> &'a [u8] {
        self.remainder
    }

    fn is_empty(&self) -> bool {
        self.remainder.is_empty()
    }
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_i16(target: &mut Vec<u8>, value: i16) {
        target.extend_from_slice(&value.to_be_bytes());
    }

    fn push_i64(target: &mut Vec<u8>, value: i64) {
        target.extend_from_slice(&value.to_be_bytes());
    }

    fn push_frame(target: &mut Vec<u8>, value: &[u8]) {
        push_i64(target, value.len() as i64);
        target.extend_from_slice(value);
    }

    fn predecessor_frame() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "drain_intent_id": "ffeeddccbbaa99887766554433221100",
            "source_intent_revision": 2,
            "source_state_digest": "a".repeat(64),
            "predecessor_claim_terminal_digest": "b".repeat(64),
            "predecessor_gateway_shard_id": "shard:0",
            "predecessor_process_instance_id": "process:old",
            "predecessor_lease_epoch": 1,
            "predecessor_runtime_build_revision": "build:1",
            "predecessor_owner_revision": 2,
            "predecessor_controller_id": "recovery:11111111111111111111111111111111:2",
            "predecessor_controller_fencing_token": 3,
            "predecessor_claim_epoch": 4,
            "predecessor_claim_revision": 1,
            "predecessor_claim_expires_at_unix_microseconds": 1000000,
            "predecessor_seal_process_instance_id": "process:old",
            "predecessor_seal_generation": 1,
            "predecessor_seal_observation_sequence": 1,
            "predecessor_claim_source_digest": "c".repeat(64)
        }))
        .unwrap()
    }

    fn transition_frame(certification: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "tenant_id": "tenant:1",
            "installation_id": "installation:1",
            "deployment_id": "deployment:1",
            "expected_revision": 1,
            "product_operation_id": "00112233445566778899aabbccddeeff",
            "product_mutation_digest": "d".repeat(64),
            "product_mutation_request_digest": "e".repeat(64),
            "drain_intent_digest": "f".repeat(64),
            "drain_intent_request_digest": "1".repeat(64),
            "slot_guild_id": "42",
            "slot_ruleset_key": "studyroom",
            "target_version": 1,
            "target_content_hash": "2".repeat(64),
            "target_binding_revision": 3,
            "target_binding_fingerprint": "3".repeat(64),
            "source_fencing_token": 3,
            "successor_fencing_token": 4,
            "successor_controller_id": "recovery:22222222222222222222222222222222:2",
            "successor_claim_revision": 2,
            "successor_intent_revision": 3,
            "successor_state_digest": "4".repeat(64),
            "certification": certification,
            "database_now_unix_microseconds": 2000000
        }))
        .unwrap()
    }

    fn seal_frame(version: i16, pre_slot_tag: i16, failed_closed_tag: i16) -> Vec<u8> {
        let mut frame = Vec::new();
        push_i16(&mut frame, version);
        push_i16(&mut frame, pre_slot_tag);
        if pre_slot_tag == PRESENT_TAG {
            push_i64(&mut frame, 1);
            push_i64(&mut frame, 2);
        }
        frame.extend_from_slice(&[7; 16]);
        for value in [1_i64, 1, 1, 10, 1, 0, 0, 0, 0, 1, 0, 0] {
            push_i64(&mut frame, value);
        }
        push_i16(&mut frame, failed_closed_tag);
        frame
    }

    fn evidence_frame(
        ready_tag: i16,
        last_resume_tag: i16,
        seal: &[u8],
        trailing: &[u8],
    ) -> Vec<u8> {
        let mut frame = b"22222222222222222222222222222222".to_vec();
        for value in [2_i64, 3, 5, 4] {
            push_i64(&mut frame, value);
        }
        push_frame(&mut frame, b"process:new");
        push_i64(&mut frame, 2);
        push_i64(&mut frame, 4);
        push_i16(&mut frame, ready_tag);
        push_i64(&mut frame, 5);
        push_i64(&mut frame, 8);
        push_i64(&mut frame, 6);
        push_i16(&mut frame, last_resume_tag);
        if last_resume_tag == PRESENT_TAG {
            push_i64(&mut frame, 7);
        }
        push_frame(&mut frame, b"process:new");
        push_i64(&mut frame, 9);
        push_i64(&mut frame, 0);
        push_i64(&mut frame, 0);
        push_frame(&mut frame, seal);
        frame.extend_from_slice(trailing);
        frame
    }

    fn projection_with_frames(frames: [&[u8]; 4]) -> Vec<u8> {
        let mut projection = Vec::new();
        push_i64(&mut projection, TERMINAL_PROJECTION_DOMAIN.len() as i64);
        projection.extend_from_slice(TERMINAL_PROJECTION_DOMAIN);
        push_i16(&mut projection, TERMINAL_PROJECTION_VERSION);
        push_i16(&mut projection, TERMINAL_PROJECTION_TAG);
        let mut framed_payload = Vec::new();
        for frame in frames {
            push_frame(&mut framed_payload, frame);
        }
        projection.extend_from_slice(&framed_payload);
        projection.extend_from_slice(&Sha256::digest(&framed_payload));
        projection
    }

    fn projection() -> Vec<u8> {
        let predecessor = predecessor_frame();
        let successor = br#"{"successor":true}"#;
        let seal = seal_frame(SEAL_BUNDLE_VERSION, ABSENT_TAG, ABSENT_TAG);
        let evidence = evidence_frame(READY_TAG, PRESENT_TAG, &seal, &[]);
        let transition = transition_frame(serde_json::json!({
            "kind": "no_operation_reserved"
        }));
        projection_with_frames([&predecessor, successor, &evidence, &transition])
    }

    #[test]
    fn exact_v3_projection_decodes_every_typed_frame() {
        let decoded = decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection(),
        )
        .unwrap();
        assert_eq!(decoded.predecessor.source_intent_revision, 2);
        assert_eq!(
            decoded.successor_state_bytes.as_ref(),
            br#"{"successor":true}"#
        );
        assert_eq!(
            decoded.closed_evidence.paused_ready_kind,
            RuntimePendingDrainSuccessionReadyKindV3::Ready
        );
        assert_eq!(decoded.closed_evidence.paused_last_resume_sequence, Some(7));
        assert_eq!(decoded.seal_evidence.pre_slot, None);
        assert_eq!(decoded.seal_evidence.seal_key, [7; 16]);
        assert!(matches!(
            decoded.transition.certification,
            RuntimePendingDrainSuccessionCertificationV3::NoOperationReserved {}
        ));
    }

    #[test]
    fn domain_version_tag_outcome_size_and_digest_are_exact() {
        let valid = projection();
        assert!(decode_pending_drain_succession_terminal_projection_v3("claimed", &valid).is_err());
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &[]
        )
        .is_err());

        let mut domain = valid.clone();
        domain[8] ^= 1;
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &domain
        )
        .is_err());

        let prefix_end = 8 + TERMINAL_PROJECTION_DOMAIN.len();
        let mut version = valid.clone();
        version[prefix_end + 1] = 2;
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &version
        )
        .is_err());

        let mut tag = valid.clone();
        tag[prefix_end + 3] = 2;
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &tag
        )
        .is_err());

        let mut digest = valid.clone();
        let last = digest.len() - 1;
        digest[last] ^= 1;
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &digest
        )
        .is_err());

        let mut appended = valid.clone();
        appended.push(0);
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &appended
        )
        .is_err());
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &valid[..valid.len() - 1]
        )
        .is_err());
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &vec![0; MAX_TERMINAL_PROJECTION_BYTES + 1]
        )
        .is_err());
    }

    #[test]
    fn four_nonempty_bounded_frames_and_exact_consumption_are_required() {
        let predecessor = predecessor_frame();
        let successor = b"successor";
        let seal = seal_frame(SEAL_BUNDLE_VERSION, ABSENT_TAG, ABSENT_TAG);
        let evidence = evidence_frame(READY_TAG, ABSENT_TAG, &seal, &[]);
        let transition = transition_frame(serde_json::json!({
            "kind": "no_operation_reserved"
        }));
        for frames in [
            [&[][..], successor.as_slice(), &evidence, &transition],
            [&predecessor, &[][..], &evidence, &transition],
            [&predecessor, successor.as_slice(), &[][..], &transition],
            [&predecessor, successor.as_slice(), &evidence, &[][..]],
        ] {
            assert!(decode_pending_drain_succession_terminal_projection_v3(
                "route_absent_acknowledged",
                &projection_with_frames(frames)
            )
            .is_err());
        }

        let oversized_predecessor = vec![b'x'; MAX_PREDECESSOR_FRAME_BYTES + 1];
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&oversized_predecessor, successor, &evidence, &transition])
        )
        .is_err());
        let oversized_evidence = vec![0; MAX_EVIDENCE_FRAME_BYTES + 1];
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&predecessor, successor, &oversized_evidence, &transition])
        )
        .is_err());
        let oversized_transition = vec![b'x'; MAX_TRANSITION_FRAME_BYTES + 1];
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&predecessor, successor, &evidence, &oversized_transition])
        )
        .is_err());
    }

    #[test]
    fn predecessor_and_transition_json_shapes_reject_missing_unknown_and_duplicate_fields() {
        let predecessor = String::from_utf8(predecessor_frame()).unwrap();
        let successor = b"successor";
        let seal = seal_frame(SEAL_BUNDLE_VERSION, ABSENT_TAG, ABSENT_TAG);
        let evidence = evidence_frame(READY_TAG, ABSENT_TAG, &seal, &[]);
        let transition = transition_frame(serde_json::json!({
            "kind": "no_operation_reserved"
        }));

        let mut missing_value = serde_json::from_str::<serde_json::Value>(&predecessor).unwrap();
        missing_value
            .as_object_mut()
            .unwrap()
            .remove("drain_intent_id");
        let missing = serde_json::to_string(&missing_value).unwrap();
        let unknown = predecessor.replacen('{', "{\"unknown\":1,", 1);
        let duplicate = predecessor.replacen(
            "\"drain_intent_id\":",
            "\"drain_intent_id\":\"duplicate\",\"drain_intent_id\":",
            1,
        );
        for invalid_predecessor in [missing, unknown, duplicate] {
            assert!(decode_pending_drain_succession_terminal_projection_v3(
                "route_absent_acknowledged",
                &projection_with_frames([
                    invalid_predecessor.as_bytes(),
                    successor,
                    &evidence,
                    &transition
                ])
            )
            .is_err());
        }

        let transition_text = String::from_utf8(transition).unwrap();
        let mut missing_value =
            serde_json::from_str::<serde_json::Value>(&transition_text).unwrap();
        missing_value.as_object_mut().unwrap().remove("tenant_id");
        let missing = serde_json::to_string(&missing_value).unwrap();
        let unknown = transition_text.replacen('{', "{\"unknown\":1,", 1);
        let duplicate = transition_text.replacen(
            "\"tenant_id\":",
            "\"tenant_id\":\"duplicate\",\"tenant_id\":",
            1,
        );
        for invalid_transition in [missing, unknown, duplicate] {
            assert!(decode_pending_drain_succession_terminal_projection_v3(
                "route_absent_acknowledged",
                &projection_with_frames([
                    predecessor.as_bytes(),
                    successor,
                    &evidence,
                    invalid_transition.as_bytes()
                ])
            )
            .is_err());
        }
    }

    #[test]
    fn certification_variants_are_closed_and_committed_remains_typed_for_semantic_rejection() {
        let predecessor = predecessor_frame();
        let successor = b"successor";
        let seal = seal_frame(SEAL_BUNDLE_VERSION, ABSENT_TAG, ABSENT_TAG);
        let evidence = evidence_frame(RESUMED_TAG, ABSENT_TAG, &seal, &[]);
        let no_attestation = transition_frame(serde_json::json!({
            "kind": "no_attestation_for_reserved_operation",
            "operation_id": "00112233445566778899aabbccddeeff",
            "intent_fingerprint": "a".repeat(64)
        }));
        let decoded = decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&predecessor, successor, &evidence, &no_attestation]),
        )
        .unwrap();
        assert!(matches!(
            decoded.transition.certification,
            RuntimePendingDrainSuccessionCertificationV3::NoAttestationForReservedOperation { .. }
        ));

        let committed = transition_frame(serde_json::json!({
            "kind": "committed_and_disconnected",
            "operation_id": "00112233445566778899aabbccddeeff",
            "serving_identity": {
                "scope": {
                    "tenant_id": "tenant:1",
                    "installation_id": "installation:1",
                    "deployment_id": "deployment:1"
                },
                "operation_id": "00112233445566778899aabbccddeeff",
                "attestation_digest": "b".repeat(64),
                "process_identity": {
                    "target": {
                        "guild_id": "42",
                        "ruleset_key": "studyroom",
                        "version": 1,
                        "content_hash": "c".repeat(64),
                        "binding_revision": 3,
                        "binding_fingerprint": "d".repeat(64)
                    },
                    "runtime_generation": 1,
                    "process_instance_id": "process:old"
                },
                "lease_epoch": 1,
                "revision": 2
            },
            "disconnected_revision": 3
        }));
        let decoded = decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&predecessor, successor, &evidence, &committed]),
        )
        .unwrap();
        assert!(matches!(
            decoded.transition.certification,
            RuntimePendingDrainSuccessionCertificationV3::CommittedAndDisconnected { .. }
        ));

        let unknown = transition_frame(serde_json::json!({
            "kind": "unknown"
        }));
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&predecessor, successor, &evidence, &unknown])
        )
        .is_err());
    }

    #[test]
    fn evidence_and_seal_tags_ranges_and_trailing_bytes_fail_closed() {
        let predecessor = predecessor_frame();
        let successor = b"successor";
        let transition = transition_frame(serde_json::json!({
            "kind": "no_operation_reserved"
        }));
        let valid_seal = seal_frame(SEAL_BUNDLE_VERSION, ABSENT_TAG, ABSENT_TAG);

        for evidence in [
            evidence_frame(9, ABSENT_TAG, &valid_seal, &[]),
            evidence_frame(READY_TAG, 9, &valid_seal, &[]),
            evidence_frame(READY_TAG, ABSENT_TAG, &valid_seal, &[0]),
            evidence_frame(
                READY_TAG,
                ABSENT_TAG,
                &seal_frame(2, ABSENT_TAG, ABSENT_TAG),
                &[],
            ),
            evidence_frame(
                READY_TAG,
                ABSENT_TAG,
                &seal_frame(SEAL_BUNDLE_VERSION, 9, ABSENT_TAG),
                &[],
            ),
            evidence_frame(
                READY_TAG,
                ABSENT_TAG,
                &seal_frame(SEAL_BUNDLE_VERSION, ABSENT_TAG, 9),
                &[],
            ),
        ] {
            assert!(decode_pending_drain_succession_terminal_projection_v3(
                "route_absent_acknowledged",
                &projection_with_frames([&predecessor, successor, &evidence, &transition])
            )
            .is_err());
        }

        let mut trailing_seal = valid_seal;
        trailing_seal.push(0);
        let evidence = evidence_frame(READY_TAG, ABSENT_TAG, &trailing_seal, &[]);
        assert!(decode_pending_drain_succession_terminal_projection_v3(
            "route_absent_acknowledged",
            &projection_with_frames([&predecessor, successor, &evidence, &transition])
        )
        .is_err());
    }
}
