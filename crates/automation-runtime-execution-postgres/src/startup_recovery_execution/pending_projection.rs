use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::closed_evidence::{decode_closed_recovery_evidence_v2, RuntimeClosedRecoveryEvidenceV2};
use super::projection::MAX_TERMINAL_PROJECTION_BYTES;
use crate::RuntimeExecutionPersistenceErrorV1;

const TERMINAL_PROJECTION_DOMAIN: &[u8] =
    b"starring.runtime.startup_recovery.pending_drain.terminal.v2";
const TERMINAL_PROJECTION_VERSION: i16 = 2;
const NO_CANDIDATE_TAG: i16 = 0;
const CLAIMED_TAG: i16 = 1;
const ACKNOWLEDGED_TAG: i16 = 2;
const PRODUCT_ROOT_VERSION: i16 = 2;
const SEAL_BUNDLE_VERSION: i16 = 2;
const ABSENT_TAG: i16 = 0;
const PRESENT_TAG: i16 = 1;
const CLAIM_STAGE_TAG: i16 = 1;
const ACKNOWLEDGEMENT_STAGE_TAG: i16 = 2;

pub(super) enum RuntimePendingDrainTerminalProjectionV2 {
    NoCandidate(RuntimeClosedRecoveryEvidenceV2),
    Claimed(Box<RuntimePendingDrainProgressedProjectionV2>),
    RouteAbsentAcknowledged(Box<RuntimePendingDrainProgressedProjectionV2>),
}

pub(super) struct RuntimePendingDrainProgressedProjectionV2 {
    pub source_state_digest: String,
    pub successor_state_bytes: Box<[u8]>,
    pub evidence: RuntimeClosedRecoveryEvidenceV2,
    pub product_root: RuntimePendingDrainProductRootV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainProductRootV2 {
    pub product_tenant_id: String,
    pub product_installation_id: String,
    pub product_deployment_id: String,
    pub product_expected_revision: i64,
    pub product_operation_id: String,
    pub drain_tenant_id: String,
    pub drain_installation_id: String,
    pub drain_deployment_id: String,
    pub drain_slot_guild_id: String,
    pub drain_slot_ruleset_key: String,
    pub drain_expected_revision: i64,
    pub drain_intent_id: String,
    pub target_guild_id: String,
    pub target_ruleset_key: String,
    pub target_version: i64,
    pub target_content_hash: String,
    pub target_binding_revision: i64,
    pub target_binding_fingerprint: String,
    pub product_mutation_bytes: Box<[u8]>,
    pub product_mutation_digest: String,
    pub drain_intent_bytes: Box<[u8]>,
    pub drain_intent_digest: String,
    pub source_intent_revision: i64,
    pub claim_action_authority_revision: i64,
    pub prior_claim_terminal_digest: Option<String>,
    pub seal: RuntimePendingDrainSealBundleV2,
    pub deployment_revision: i64,
    pub deployment_phase: String,
    pub source_last_fencing_token: i64,
    pub source_last_controller_id: String,
    pub source_deployment_snapshot_bytes: Box<[u8]>,
    pub successor_last_fencing_token: i64,
    pub successor_last_controller_id: String,
    pub successor_deployment_snapshot_bytes: Box<[u8]>,
    pub transitioned_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainSealBundleV2 {
    pub pre_slot: Option<RuntimePendingDrainPreSlotEvidenceV2>,
    pub seal_key: [u8; 16],
    pub seal_generation: i64,
    pub post_admission_generation: i64,
    pub post_slot_observation_sequence: i64,
    pub pre_global_sequence: i64,
    pub pre_retained_slots: i64,
    pub pre_retained_empty: i64,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RuntimePendingDrainPreSlotEvidenceV2 {
    pub admission_generation: i64,
    pub observation_sequence: i64,
}

pub(super) fn decode_pending_drain_terminal_projection_v2(
    terminal_outcome_name: &str,
    projection: &[u8],
) -> Result<RuntimePendingDrainTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1> {
    if projection.is_empty() || projection.len() > MAX_TERMINAL_PROJECTION_BYTES {
        return Err(invalid());
    }
    let expected_tag = match terminal_outcome_name {
        "no_candidate" => NO_CANDIDATE_TAG,
        "claimed" => CLAIMED_TAG,
        "route_absent_acknowledged" => ACKNOWLEDGED_TAG,
        _ => return Err(invalid()),
    };
    let prefix = projection_prefix(expected_tag);
    let remainder = projection
        .strip_prefix(prefix.as_slice())
        .ok_or_else(invalid)?;
    let digest_start = remainder.len().checked_sub(32).ok_or_else(invalid)?;
    let (framed_payload, persisted_digest) = remainder.split_at(digest_start);
    if Sha256::digest(framed_payload).as_slice() != persisted_digest {
        return Err(invalid());
    }
    let mut cursor = Cursor::new(framed_payload);
    let source_digest_frame = cursor.take_frame()?;
    let successor_state_frame = cursor.take_frame()?;
    let evidence_frame = cursor.take_nonempty_frame()?;
    let product_root_frame = cursor.take_frame()?;
    if !cursor.is_empty() {
        return Err(invalid());
    }
    let evidence = decode_closed_recovery_evidence_v2(evidence_frame)?;
    match terminal_outcome_name {
        "no_candidate"
            if source_digest_frame.is_empty()
                && successor_state_frame.is_empty()
                && product_root_frame.is_empty() =>
        {
            Ok(RuntimePendingDrainTerminalProjectionV2::NoCandidate(
                evidence,
            ))
        }
        "claimed" | "route_absent_acknowledged"
            if !source_digest_frame.is_empty()
                && !successor_state_frame.is_empty()
                && !product_root_frame.is_empty() =>
        {
            let progressed = Box::new(RuntimePendingDrainProgressedProjectionV2 {
                source_state_digest: decode_digest_text(source_digest_frame)?,
                successor_state_bytes: successor_state_frame.to_vec().into_boxed_slice(),
                evidence,
                product_root: decode_product_root(product_root_frame, expected_tag)?,
            });
            if terminal_outcome_name == "claimed" {
                Ok(RuntimePendingDrainTerminalProjectionV2::Claimed(progressed))
            } else {
                Ok(RuntimePendingDrainTerminalProjectionV2::RouteAbsentAcknowledged(progressed))
            }
        }
        _ => Err(invalid()),
    }
}

fn decode_product_root(
    frame: &[u8],
    expected_outcome_tag: i16,
) -> Result<RuntimePendingDrainProductRootV2, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    if cursor.take_i16()? != PRODUCT_ROOT_VERSION {
        return Err(invalid());
    }
    let product_tenant_id = cursor.take_nonempty_text_frame()?;
    let product_installation_id = cursor.take_nonempty_text_frame()?;
    let product_deployment_id = cursor.take_nonempty_text_frame()?;
    let product_expected_revision = cursor.take_positive_i64()?;
    let product_operation_id = cursor.take_nonempty_text_frame()?;
    let drain_tenant_id = cursor.take_nonempty_text_frame()?;
    let drain_installation_id = cursor.take_nonempty_text_frame()?;
    let drain_deployment_id = cursor.take_nonempty_text_frame()?;
    let drain_slot_guild_id = cursor.take_nonempty_text_frame()?;
    let drain_slot_ruleset_key = cursor.take_nonempty_text_frame()?;
    let drain_expected_revision = cursor.take_positive_i64()?;
    let drain_intent_id = cursor.take_nonempty_text_frame()?;
    let target_guild_id = cursor.take_nonempty_text_frame()?;
    let target_ruleset_key = cursor.take_nonempty_text_frame()?;
    let target_version = cursor.take_positive_i64()?;
    let target_content_hash = cursor.take_nonempty_text_frame()?;
    let target_binding_revision = cursor.take_positive_i64()?;
    let target_binding_fingerprint = cursor.take_nonempty_text_frame()?;
    let product_mutation_bytes = cursor.take_nonempty_frame()?.to_vec().into_boxed_slice();
    let product_mutation_digest = decode_digest_text(cursor.take_nonempty_frame()?)?;
    let drain_intent_bytes = cursor.take_nonempty_frame()?.to_vec().into_boxed_slice();
    let drain_intent_digest = decode_digest_text(cursor.take_nonempty_frame()?)?;
    let source_intent_revision = cursor.take_positive_i64()?;
    let claim_action_authority_revision = cursor.take_positive_i64()?;
    let stage_tag = cursor.take_i16()?;
    let prior_claim_terminal_digest = match cursor.take_i16()? {
        ABSENT_TAG => None,
        PRESENT_TAG => Some(decode_digest_text(cursor.take_nonempty_frame()?)?),
        _ => return Err(invalid()),
    };
    let seal = decode_seal_bundle(cursor.take_nonempty_frame()?)?;
    let product_root = RuntimePendingDrainProductRootV2 {
        product_tenant_id,
        product_installation_id,
        product_deployment_id,
        product_expected_revision,
        product_operation_id,
        drain_tenant_id,
        drain_installation_id,
        drain_deployment_id,
        drain_slot_guild_id,
        drain_slot_ruleset_key,
        drain_expected_revision,
        drain_intent_id,
        target_guild_id,
        target_ruleset_key,
        target_version,
        target_content_hash,
        target_binding_revision,
        target_binding_fingerprint,
        product_mutation_bytes,
        product_mutation_digest,
        drain_intent_bytes,
        drain_intent_digest,
        source_intent_revision,
        claim_action_authority_revision,
        prior_claim_terminal_digest,
        seal,
        deployment_revision: cursor.take_positive_i64()?,
        deployment_phase: cursor.take_nonempty_text_frame()?,
        source_last_fencing_token: cursor.take_positive_i64()?,
        source_last_controller_id: cursor.take_nonempty_text_frame()?,
        source_deployment_snapshot_bytes: cursor.take_nonempty_frame()?.to_vec().into_boxed_slice(),
        successor_last_fencing_token: cursor.take_positive_i64()?,
        successor_last_controller_id: cursor.take_nonempty_text_frame()?,
        successor_deployment_snapshot_bytes: cursor
            .take_nonempty_frame()?
            .to_vec()
            .into_boxed_slice(),
        transitioned_at: DateTime::from_timestamp_micros(cursor.take_i64()?).ok_or_else(invalid)?,
    };
    if !cursor.is_empty() {
        return Err(invalid());
    }
    match (expected_outcome_tag, stage_tag) {
        (CLAIMED_TAG, CLAIM_STAGE_TAG) if product_root.prior_claim_terminal_digest.is_none() => {}
        (ACKNOWLEDGED_TAG, ACKNOWLEDGEMENT_STAGE_TAG)
            if product_root.prior_claim_terminal_digest.is_some() => {}
        _ => return Err(invalid()),
    }
    Ok(product_root)
}

fn decode_seal_bundle(
    frame: &[u8],
) -> Result<RuntimePendingDrainSealBundleV2, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    if cursor.take_i16()? != SEAL_BUNDLE_VERSION {
        return Err(invalid());
    }
    let pre_slot = match cursor.take_i16()? {
        ABSENT_TAG => None,
        PRESENT_TAG => Some(RuntimePendingDrainPreSlotEvidenceV2 {
            admission_generation: cursor.take_positive_i64()?,
            observation_sequence: cursor.take_positive_i64()?,
        }),
        _ => return Err(invalid()),
    };
    let seal_key = cursor.take_fixed::<16>()?;
    let seal_generation = cursor.take_positive_i64()?;
    let post_admission_generation = cursor.take_positive_i64()?;
    let post_slot_observation_sequence = cursor.take_positive_i64()?;
    if cursor.take_i16()? != ABSENT_TAG || cursor.take_i64()? != 0 {
        return Err(invalid());
    }
    let seal = RuntimePendingDrainSealBundleV2 {
        pre_slot,
        seal_key,
        seal_generation,
        post_admission_generation,
        post_slot_observation_sequence,
        pre_global_sequence: cursor.take_positive_i64()?,
        pre_retained_slots: cursor.take_nonnegative_i64()?,
        pre_retained_empty: cursor.take_nonnegative_i64()?,
        post_global_sequence: cursor.take_positive_i64()?,
        post_retained_slots: cursor.take_nonnegative_i64()?,
        post_retained_empty: cursor.take_nonnegative_i64()?,
        post_staged: cursor.take_nonnegative_i64()?,
        post_serving: cursor.take_nonnegative_i64()?,
        post_draining: cursor.take_nonnegative_i64()?,
        post_sealed: cursor.take_nonnegative_i64()?,
        post_active: cursor.take_nonnegative_i64()?,
        post_failed_closed_slots: cursor.take_nonnegative_i64()?,
    };
    if cursor.take_i16()? != ABSENT_TAG || !cursor.is_empty() {
        return Err(invalid());
    }
    Ok(seal)
}

fn decode_digest_text(frame: &[u8]) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
    if frame.len() != 64
        || !frame
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(invalid());
    }
    std::str::from_utf8(frame)
        .map(str::to_owned)
        .map_err(|_| invalid())
}

fn projection_prefix(tag: i16) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(TERMINAL_PROJECTION_DOMAIN.len() + 12);
    prefix.extend_from_slice(&(TERMINAL_PROJECTION_DOMAIN.len() as i64).to_be_bytes());
    prefix.extend_from_slice(TERMINAL_PROJECTION_DOMAIN);
    prefix.extend_from_slice(&TERMINAL_PROJECTION_VERSION.to_be_bytes());
    prefix.extend_from_slice(&tag.to_be_bytes());
    prefix
}

struct Cursor<'a> {
    remainder: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(value: &'a [u8]) -> Self {
        Self { remainder: value }
    }

    fn take_i16(&mut self) -> Result<i16, RuntimeExecutionPersistenceErrorV1> {
        Ok(i16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| invalid())?,
        ))
    }

    fn take_i64(&mut self) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
        Ok(i64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| invalid())?,
        ))
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

    fn take_frame(&mut self) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let length = usize::try_from(self.take_i64()?).map_err(|_| invalid())?;
        if length > MAX_TERMINAL_PROJECTION_BYTES {
            return Err(invalid());
        }
        self.take(length)
    }

    fn take_nonempty_frame(&mut self) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let frame = self.take_frame()?;
        if frame.is_empty() {
            Err(invalid())
        } else {
            Ok(frame)
        }
    }

    fn take_nonempty_text_frame(&mut self) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
        std::str::from_utf8(self.take_nonempty_frame()?)
            .map(str::to_owned)
            .map_err(|_| invalid())
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let (value, remainder) = self
            .remainder
            .split_at_checked(length)
            .ok_or_else(invalid)?;
        self.remainder = remainder;
        Ok(value)
    }

    fn take_fixed<const LENGTH: usize>(
        &mut self,
    ) -> Result<[u8; LENGTH], RuntimeExecutionPersistenceErrorV1> {
        self.take(LENGTH)?.try_into().map_err(|_| invalid())
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

    fn push_frame(target: &mut Vec<u8>, value: &[u8]) {
        target.extend_from_slice(&(value.len() as i64).to_be_bytes());
        target.extend_from_slice(value);
    }

    fn evidence_frame(last_resume: Option<i64>) -> Vec<u8> {
        let mut frame = Vec::new();
        push_frame(&mut frame, b"process:gateway");
        for value in [2_i64, 4] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame.extend_from_slice(&1_i16.to_be_bytes());
        for value in [5_i64, 8, 6] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        match last_resume {
            None => frame.extend_from_slice(&0_i16.to_be_bytes()),
            Some(value) => {
                frame.extend_from_slice(&1_i16.to_be_bytes());
                frame.extend_from_slice(&value.to_be_bytes());
            }
        }
        push_frame(&mut frame, b"process:gateway");
        for value in [9_i64, 0, 0] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame
    }

    fn seal_bundle_frame() -> Vec<u8> {
        let mut frame = SEAL_BUNDLE_VERSION.to_be_bytes().to_vec();
        frame.extend_from_slice(&PRESENT_TAG.to_be_bytes());
        frame.extend_from_slice(&4_i64.to_be_bytes());
        frame.extend_from_slice(&5_i64.to_be_bytes());
        frame.extend_from_slice(&[
            0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
            0x11, 0x00,
        ]);
        for value in [12_i64, 5, 6] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame.extend_from_slice(&ABSENT_TAG.to_be_bytes());
        frame.extend_from_slice(&0_i64.to_be_bytes());
        for value in [9_i64, 2, 2, 10, 2, 1, 0, 0, 0, 1, 0, 0] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame.extend_from_slice(&ABSENT_TAG.to_be_bytes());
        frame
    }

    fn product_root_frame(stage_tag: i16) -> Vec<u8> {
        let mut frame = PRODUCT_ROOT_VERSION.to_be_bytes().to_vec();
        for value in ["tenant:1", "installation:1", "deployment:1"] {
            push_frame(&mut frame, value.as_bytes());
        }
        frame.extend_from_slice(&7_i64.to_be_bytes());
        push_frame(&mut frame, b"product:1");
        for value in [
            "tenant:1",
            "installation:1",
            "deployment:1",
            "9223372036854775808",
            "studyroom",
        ] {
            push_frame(&mut frame, value.as_bytes());
        }
        frame.extend_from_slice(&7_i64.to_be_bytes());
        push_frame(&mut frame, b"drain:1");
        push_frame(&mut frame, b"9223372036854775808");
        push_frame(&mut frame, b"studyroom");
        frame.extend_from_slice(&3_i64.to_be_bytes());
        push_frame(
            &mut frame,
            b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        frame.extend_from_slice(&4_i64.to_be_bytes());
        push_frame(
            &mut frame,
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        push_frame(&mut frame, br#"{"format_version":2}"#);
        push_frame(
            &mut frame,
            b"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        );
        push_frame(&mut frame, br#"{"format_version":2}"#);
        push_frame(
            &mut frame,
            b"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        );
        frame.extend_from_slice(
            &(if stage_tag == CLAIM_STAGE_TAG {
                1_i64
            } else {
                2_i64
            })
            .to_be_bytes(),
        );
        frame.extend_from_slice(&5_i64.to_be_bytes());
        frame.extend_from_slice(&stage_tag.to_be_bytes());
        if stage_tag == CLAIM_STAGE_TAG {
            frame.extend_from_slice(&ABSENT_TAG.to_be_bytes());
        } else {
            frame.extend_from_slice(&PRESENT_TAG.to_be_bytes());
            push_frame(
                &mut frame,
                b"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            );
        }
        push_frame(&mut frame, &seal_bundle_frame());
        frame.extend_from_slice(&7_i64.to_be_bytes());
        push_frame(&mut frame, b"requested");
        frame.extend_from_slice(&8_i64.to_be_bytes());
        push_frame(&mut frame, b"controller:old");
        push_frame(&mut frame, br#"{"revision":7}"#);
        frame.extend_from_slice(&9_i64.to_be_bytes());
        push_frame(&mut frame, b"recovery:0123456789abcdef0123456789abcdef:5");
        push_frame(&mut frame, br#"{"revision":7,"last_fencing_token":9}"#);
        frame.extend_from_slice(&123_000_000_i64.to_be_bytes());
        frame
    }

    fn projection(tag: i16, frames: [&[u8]; 4]) -> Vec<u8> {
        let mut projection = projection_prefix(tag);
        let mut payload = Vec::new();
        for frame in frames {
            push_frame(&mut payload, frame);
        }
        projection.extend_from_slice(&payload);
        projection.extend_from_slice(&Sha256::digest(&payload));
        projection
    }

    #[test]
    fn every_terminal_shape_is_exact_and_full_root_is_decoded() {
        let evidence = evidence_frame(Some(7));
        let no_candidate = projection(NO_CANDIDATE_TAG, [b"", b"", &evidence, b""]);
        let RuntimePendingDrainTerminalProjectionV2::NoCandidate(decoded) =
            decode_pending_drain_terminal_projection_v2("no_candidate", &no_candidate).unwrap()
        else {
            panic!("expected no candidate")
        };
        assert_eq!(decoded.paused_last_resume_sequence, Some(7));

        for (outcome, tag) in [
            ("claimed", CLAIMED_TAG),
            ("route_absent_acknowledged", ACKNOWLEDGED_TAG),
        ] {
            let root = product_root_frame(if tag == CLAIMED_TAG {
                CLAIM_STAGE_TAG
            } else {
                ACKNOWLEDGEMENT_STAGE_TAG
            });
            let encoded = projection(
                tag,
                [
                    b"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                    br#"{"format_version":2}"#,
                    &evidence,
                    &root,
                ],
            );
            let progressed =
                decode_pending_drain_terminal_projection_v2(outcome, &encoded).unwrap();
            let progressed = match progressed {
                RuntimePendingDrainTerminalProjectionV2::Claimed(value)
                | RuntimePendingDrainTerminalProjectionV2::RouteAbsentAcknowledged(value) => value,
                RuntimePendingDrainTerminalProjectionV2::NoCandidate(_) => {
                    panic!("expected progress")
                }
            };
            assert_eq!(progressed.product_root.product_tenant_id, "tenant:1");
            assert_eq!(progressed.product_root.drain_slot_ruleset_key, "studyroom");
            assert_eq!(progressed.product_root.source_last_fencing_token, 8);
            assert_eq!(progressed.product_root.successor_last_fencing_token, 9);
            assert_eq!(progressed.product_root.seal.seal_generation, 12);
            assert_eq!(
                progressed.product_root.seal.seal_key,
                [
                    0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33,
                    0x22, 0x11, 0x00,
                ]
            );
            assert_eq!(
                progressed.product_root.successor_last_controller_id,
                "recovery:0123456789abcdef0123456789abcdef:5"
            );
            assert_eq!(
                progressed.product_root.transitioned_at,
                DateTime::from_timestamp(123, 0).unwrap()
            );
        }
    }

    #[test]
    fn outcome_substitution_truncation_append_and_digest_forgery_fail_closed() {
        let evidence = evidence_frame(None);
        let root = product_root_frame(CLAIM_STAGE_TAG);
        let encoded = projection(
            CLAIMED_TAG,
            [
                b"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                br#"{"format_version":2}"#,
                &evidence,
                &root,
            ],
        );
        assert!(
            decode_pending_drain_terminal_projection_v2("route_absent_acknowledged", &encoded)
                .is_err()
        );
        for malformed in [
            encoded[..encoded.len() - 1].to_vec(),
            {
                let mut value = encoded.clone();
                value.push(0);
                value
            },
            {
                let mut value = encoded.clone();
                let last = value.len() - 1;
                value[last] ^= 1;
                value
            },
        ] {
            assert!(decode_pending_drain_terminal_projection_v2("claimed", &malformed).is_err());
        }
    }

    #[test]
    fn empty_progress_frames_noncanonical_digests_and_root_tampering_are_rejected() {
        let evidence = evidence_frame(None);
        let root = product_root_frame(CLAIM_STAGE_TAG);
        for frames in [
            [b"".as_slice(), br#"{}"#, &evidence, &root],
            [
                b"EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE",
                br#"{}"#,
                &evidence,
                &root,
            ],
            [
                b"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                b"",
                &evidence,
                &root,
            ],
        ] {
            let encoded = projection(CLAIMED_TAG, frames);
            assert!(decode_pending_drain_terminal_projection_v2("claimed", &encoded).is_err());
        }

        let mut trailing_root = root.clone();
        trailing_root.push(0);
        let encoded = projection(
            CLAIMED_TAG,
            [
                b"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                br#"{}"#,
                &evidence,
                &trailing_root,
            ],
        );
        assert!(decode_pending_drain_terminal_projection_v2("claimed", &encoded).is_err());
    }

    #[test]
    fn seal_bundle_tags_counts_and_exact_length_fail_closed() {
        let valid = seal_bundle_frame();
        assert!(decode_seal_bundle(&valid).is_ok());
        for malformed in [
            {
                let mut value = valid.clone();
                value[1] = 3;
                value
            },
            {
                let mut value = valid.clone();
                value[3] = 2;
                value
            },
            {
                let mut value = valid.clone();
                let post_route_tag = 2 + 2 + 8 + 8 + 16 + 8 + 8 + 8;
                value[post_route_tag + 1] = 1;
                value
            },
            {
                let mut value = valid.clone();
                let post_active = 2 + 2 + 8 + 8 + 16 + 8 + 8 + 8 + 2;
                value[post_active + 7] = 1;
                value
            },
            valid[..valid.len() - 1].to_vec(),
            {
                let mut value = valid.clone();
                value.push(0);
                value
            },
        ] {
            assert!(decode_seal_bundle(&malformed).is_err());
        }
    }
}
