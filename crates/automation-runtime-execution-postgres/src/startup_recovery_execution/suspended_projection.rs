use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::closed_evidence::{decode_closed_recovery_evidence_v2, RuntimeClosedRecoveryEvidenceV2};
use super::projection::MAX_TERMINAL_PROJECTION_BYTES;
use crate::RuntimeExecutionPersistenceErrorV1;

const TERMINAL_PROJECTION_DOMAIN: &[u8] =
    b"starring.runtime.startup_recovery.suspended_local_effect.terminal.v2";
const TERMINAL_PROJECTION_VERSION: i16 = 2;
const NO_CANDIDATE_TAG: i16 = 0;
const PROGRESSED_TAG: i16 = 1;
const ABSENT_TAG: i16 = 0;
const PRESENT_TAG: i16 = 1;
const POSTGRES_EPOCH_UNIX_MICROSECONDS: i64 = 946_684_800_000_000;

#[cfg(test)]
const READY_KIND_TAG: i16 = 1;

pub(super) enum RuntimeSuspendedStartupRecoveryTerminalProjectionV2 {
    NoCandidate(RuntimeClosedRecoveryEvidenceV2),
    Progressed(Box<RuntimeSuspendedStartupRecoveryProgressedProjectionV2>),
}

pub(super) struct RuntimeSuspendedStartupRecoveryProgressedProjectionV2 {
    pub root: RuntimeSuspendedStartupRecoveryRootV2,
    pub source: RuntimeSuspendedStartupRecoverySidecarV2,
    pub successor: RuntimeSuspendedStartupRecoverySidecarV2,
    pub provenance_bytes: Box<[u8]>,
    pub evidence: RuntimeClosedRecoveryEvidenceV2,
}

pub(super) struct RuntimeSuspendedStartupRecoveryRootV2 {
    pub suspension_id: String,
    pub tenant_id: String,
    pub installation_id: String,
    pub deployment_id: String,
    pub deployment_revision: i64,
    pub convergence_attempt: i64,
    pub request_digest: [u8; 32],
    pub request_bytes: Box<[u8]>,
    pub deployment_convergence_attempt: i64,
    pub deployment_last_controller_id: Option<String>,
    pub deployment_last_fencing_token: Option<i64>,
    pub deployment_snapshot_format_version: i16,
    pub deployment_snapshot_bytes: Box<[u8]>,
}

pub(super) struct RuntimeSuspendedStartupRecoverySidecarV2 {
    pub suspension_id: String,
    pub request_digest: [u8; 32],
    pub sidecar_revision: i64,
    pub slot_guild_id: String,
    pub slot_ruleset_key: String,
    pub local_effect_kind: String,
    pub local_effect_bytes: Box<[u8]>,
    pub drain_obligation_kind: String,
    pub drain_obligation_bytes: Box<[u8]>,
    pub suspended_at: DateTime<Utc>,
}

pub(super) fn decode_suspended_terminal_projection_v2(
    terminal_outcome_name: &str,
    projection: &[u8],
) -> Result<RuntimeSuspendedStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1>
{
    if projection.is_empty() || projection.len() > MAX_TERMINAL_PROJECTION_BYTES {
        return Err(invalid());
    }
    let prefix = projection_prefix(match terminal_outcome_name {
        "no_candidate" => NO_CANDIDATE_TAG,
        "progressed" => PROGRESSED_TAG,
        _ => return Err(invalid()),
    });
    let remainder = projection
        .strip_prefix(prefix.as_slice())
        .ok_or_else(invalid)?;
    let mut cursor = Cursor::new(remainder);
    match terminal_outcome_name {
        "no_candidate" => decode_no_candidate(&mut cursor),
        "progressed" => decode_progressed(&mut cursor),
        _ => Err(invalid()),
    }
}

fn decode_no_candidate(
    cursor: &mut Cursor<'_>,
) -> Result<RuntimeSuspendedStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1>
{
    let evidence_frame = cursor.take_nonempty_frame()?;
    let evidence = decode_closed_recovery_evidence_v2(evidence_frame)?;
    let persisted_digest = cursor.take_fixed::<32>()?;
    if !cursor.is_empty() || Sha256::digest(evidence_frame).as_slice() != persisted_digest {
        return Err(invalid());
    }
    Ok(RuntimeSuspendedStartupRecoveryTerminalProjectionV2::NoCandidate(evidence))
}

fn decode_progressed(
    cursor: &mut Cursor<'_>,
) -> Result<RuntimeSuspendedStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1>
{
    let root_frame = cursor.take_nonempty_frame()?;
    let source_frame = cursor.take_nonempty_frame()?;
    let successor_frame = cursor.take_nonempty_frame()?;
    let provenance_bytes = cursor.take_nonempty_frame()?;
    let evidence_frame = cursor.take_nonempty_frame()?;
    let persisted_digest = cursor.take_fixed::<32>()?;
    if !cursor.is_empty() {
        return Err(invalid());
    }
    let mut proof = Sha256::new();
    for frame in [
        root_frame,
        source_frame,
        successor_frame,
        provenance_bytes,
        evidence_frame,
    ] {
        proof.update(frame);
    }
    if proof.finalize().as_slice() != persisted_digest {
        return Err(invalid());
    }
    Ok(
        RuntimeSuspendedStartupRecoveryTerminalProjectionV2::Progressed(Box::new(
            RuntimeSuspendedStartupRecoveryProgressedProjectionV2 {
                root: decode_root(root_frame)?,
                source: decode_sidecar(source_frame)?,
                successor: decode_sidecar(successor_frame)?,
                provenance_bytes: provenance_bytes.to_vec().into_boxed_slice(),
                evidence: decode_closed_recovery_evidence_v2(evidence_frame)?,
            },
        )),
    )
}

fn decode_root(
    frame: &[u8],
) -> Result<RuntimeSuspendedStartupRecoveryRootV2, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    let root = RuntimeSuspendedStartupRecoveryRootV2 {
        suspension_id: cursor.take_nonempty_text_frame()?,
        tenant_id: cursor.take_nonempty_text_frame()?,
        installation_id: cursor.take_nonempty_text_frame()?,
        deployment_id: cursor.take_nonempty_text_frame()?,
        deployment_revision: cursor.take_i64()?,
        convergence_attempt: cursor.take_i64()?,
        request_digest: cursor.take_fixed::<32>()?,
        request_bytes: cursor.take_nonempty_frame()?.to_vec().into_boxed_slice(),
        deployment_convergence_attempt: cursor.take_i64()?,
        deployment_last_controller_id: cursor.take_optional_nonempty_text_frame()?,
        deployment_last_fencing_token: cursor.take_optional_positive_i64()?,
        deployment_snapshot_format_version: cursor.take_i16()?,
        deployment_snapshot_bytes: cursor.take_nonempty_frame()?.to_vec().into_boxed_slice(),
    };
    if !cursor.is_empty()
        || root.deployment_convergence_attempt <= 0
        || root.deployment_snapshot_format_version != 1
        || root.deployment_last_controller_id.is_none()
            != root.deployment_last_fencing_token.is_none()
    {
        return Err(invalid());
    }
    Ok(root)
}

fn decode_sidecar(
    frame: &[u8],
) -> Result<RuntimeSuspendedStartupRecoverySidecarV2, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    let sidecar = RuntimeSuspendedStartupRecoverySidecarV2 {
        suspension_id: cursor.take_nonempty_text_frame()?,
        request_digest: cursor.take_fixed::<32>()?,
        sidecar_revision: cursor.take_i64()?,
        slot_guild_id: cursor.take_nonempty_text_frame()?,
        slot_ruleset_key: cursor.take_nonempty_text_frame()?,
        local_effect_kind: cursor.take_nonempty_text_frame()?,
        local_effect_bytes: cursor.take_nonempty_frame()?.to_vec().into_boxed_slice(),
        drain_obligation_kind: cursor.take_nonempty_text_frame()?,
        drain_obligation_bytes: cursor.take_nonempty_frame()?.to_vec().into_boxed_slice(),
        suspended_at: postgres_timestamp(cursor.take_i64()?)?,
    };
    if !cursor.is_empty() {
        return Err(invalid());
    }
    Ok(sidecar)
}

fn projection_prefix(tag: i16) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(TERMINAL_PROJECTION_DOMAIN.len() + 12);
    prefix.extend_from_slice(&(TERMINAL_PROJECTION_DOMAIN.len() as i64).to_be_bytes());
    prefix.extend_from_slice(TERMINAL_PROJECTION_DOMAIN);
    prefix.extend_from_slice(&TERMINAL_PROJECTION_VERSION.to_be_bytes());
    prefix.extend_from_slice(&tag.to_be_bytes());
    prefix
}

fn postgres_timestamp(value: i64) -> Result<DateTime<Utc>, RuntimeExecutionPersistenceErrorV1> {
    value
        .checked_add(POSTGRES_EPOCH_UNIX_MICROSECONDS)
        .and_then(DateTime::from_timestamp_micros)
        .ok_or_else(invalid)
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

    fn take_nonempty_frame(&mut self) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let length = usize::try_from(self.take_i64()?).map_err(|_| invalid())?;
        if length == 0 || length > MAX_TERMINAL_PROJECTION_BYTES {
            return Err(invalid());
        }
        self.take(length)
    }

    fn take_nonempty_text_frame(&mut self) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
        std::str::from_utf8(self.take_nonempty_frame()?)
            .map(str::to_owned)
            .map_err(|_| invalid())
    }

    fn take_optional_nonempty_text_frame(
        &mut self,
    ) -> Result<Option<String>, RuntimeExecutionPersistenceErrorV1> {
        match self.take_i16()? {
            ABSENT_TAG => Ok(None),
            PRESENT_TAG => self.take_nonempty_text_frame().map(Some),
            _ => Err(invalid()),
        }
    }

    fn take_optional_positive_i64(
        &mut self,
    ) -> Result<Option<i64>, RuntimeExecutionPersistenceErrorV1> {
        match self.take_i16()? {
            ABSENT_TAG => Ok(None),
            PRESENT_TAG => self.take_positive_i64().map(Some),
            _ => Err(invalid()),
        }
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
        push_frame(&mut frame, b"process:1");
        for value in [3_i64, 4] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame.extend_from_slice(&READY_KIND_TAG.to_be_bytes());
        for value in [5_i64, 8, 6] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        match last_resume {
            None => frame.extend_from_slice(&ABSENT_TAG.to_be_bytes()),
            Some(sequence) => {
                frame.extend_from_slice(&PRESENT_TAG.to_be_bytes());
                frame.extend_from_slice(&sequence.to_be_bytes());
            }
        }
        push_frame(&mut frame, b"process:1");
        for value in [9_i64, 2, 2] {
            frame.extend_from_slice(&value.to_be_bytes());
        }
        frame
    }

    fn root_frame() -> Vec<u8> {
        let mut frame = Vec::new();
        for value in [
            b"00112233445566778899aabbccddeeff".as_slice(),
            b"tenant:1",
            b"installation:1",
            b"deployment:1",
        ] {
            push_frame(&mut frame, value);
        }
        frame.extend_from_slice(&7_i64.to_be_bytes());
        frame.extend_from_slice(&8_i64.to_be_bytes());
        frame.extend_from_slice(&[1; 32]);
        push_frame(&mut frame, b"{}");
        frame.extend_from_slice(&8_i64.to_be_bytes());
        frame.extend_from_slice(&PRESENT_TAG.to_be_bytes());
        push_frame(&mut frame, b"controller:1");
        frame.extend_from_slice(&PRESENT_TAG.to_be_bytes());
        frame.extend_from_slice(&9_i64.to_be_bytes());
        frame.extend_from_slice(&1_i16.to_be_bytes());
        push_frame(&mut frame, b"{}");
        frame
    }

    fn sidecar_frame(revision: i64) -> Vec<u8> {
        let mut frame = Vec::new();
        push_frame(&mut frame, b"00112233445566778899aabbccddeeff");
        frame.extend_from_slice(&[1; 32]);
        frame.extend_from_slice(&revision.to_be_bytes());
        for value in [
            b"7".as_slice(),
            b"studyroom",
            b"exact_route",
            b"{}",
            b"exact_local_route",
            b"{}",
        ] {
            push_frame(&mut frame, value);
        }
        frame.extend_from_slice(&0_i64.to_be_bytes());
        frame
    }

    fn no_candidate(last_resume: Option<i64>) -> Vec<u8> {
        let evidence = evidence_frame(last_resume);
        let mut projection = projection_prefix(NO_CANDIDATE_TAG);
        push_frame(&mut projection, &evidence);
        projection.extend_from_slice(&Sha256::digest(&evidence));
        projection
    }

    fn progressed() -> Vec<u8> {
        let frames = [
            root_frame(),
            sidecar_frame(1),
            sidecar_frame(2),
            br#"{"kind":"closed_recovery"}"#.to_vec(),
            evidence_frame(Some(7)),
        ];
        let mut projection = projection_prefix(PROGRESSED_TAG);
        let mut proof = Sha256::new();
        for frame in &frames {
            push_frame(&mut projection, frame);
            proof.update(frame);
        }
        projection.extend_from_slice(&proof.finalize());
        projection
    }

    #[test]
    fn evidence_projection_is_exact_for_both_closed_sequence_shapes() {
        for last_resume in [None, Some(7)] {
            let decoded =
                decode_suspended_terminal_projection_v2("no_candidate", &no_candidate(last_resume))
                    .unwrap();
            let RuntimeSuspendedStartupRecoveryTerminalProjectionV2::NoCandidate(evidence) =
                decoded
            else {
                panic!();
            };
            assert_eq!(evidence.paused_last_resume_sequence, last_resume);
            assert_eq!(evidence.paused_ready_kind, "ready");
            assert_eq!(evidence.registry_observation_sequence, 9);
        }
    }

    #[test]
    fn progressed_projection_preserves_every_independent_frame() {
        let decoded = decode_suspended_terminal_projection_v2("progressed", &progressed()).unwrap();
        let RuntimeSuspendedStartupRecoveryTerminalProjectionV2::Progressed(projection) = decoded
        else {
            panic!();
        };
        assert_eq!(
            projection.root.suspension_id,
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(projection.source.sidecar_revision, 1);
        assert_eq!(projection.successor.sidecar_revision, 2);
        assert_eq!(projection.root.deployment_convergence_attempt, 8);
        assert_eq!(
            projection.root.deployment_last_controller_id.as_deref(),
            Some("controller:1")
        );
        assert_eq!(projection.root.deployment_last_fencing_token, Some(9));
        assert_eq!(projection.root.deployment_snapshot_format_version, 1);
        assert_eq!(
            projection.provenance_bytes.as_ref(),
            br#"{"kind":"closed_recovery"}"#
        );
        assert_eq!(projection.evidence.paused_last_resume_sequence, Some(7));
    }

    #[test]
    fn projection_rejects_truncation_append_flip_and_outcome_substitution() {
        for valid in [no_candidate(None), progressed()] {
            for cut in 0..valid.len() {
                assert!(decode_suspended_terminal_projection_v2(
                    if valid == progressed() {
                        "progressed"
                    } else {
                        "no_candidate"
                    },
                    &valid[..cut],
                )
                .is_err());
            }
            let outcome = if valid == progressed() {
                "progressed"
            } else {
                "no_candidate"
            };
            let mut appended = valid.clone();
            appended.push(0);
            assert!(decode_suspended_terminal_projection_v2(outcome, &appended).is_err());
            let mut flipped = valid.clone();
            let index = flipped.len() / 2;
            flipped[index] ^= 1;
            assert!(decode_suspended_terminal_projection_v2(outcome, &flipped).is_err());
            let substituted = if outcome == "progressed" {
                "no_candidate"
            } else {
                "progressed"
            };
            assert!(decode_suspended_terminal_projection_v2(substituted, &valid).is_err());
        }
    }

    #[test]
    fn evidence_rejects_unknown_ready_presence_and_sequence_shapes() {
        let valid = evidence_frame(Some(7));
        let ready_offset = 8 + "process:1".len() + 16;
        let presence_offset = ready_offset + 2 + 24;
        for (offset, value) in [(ready_offset, 3_i16), (presence_offset, 2_i16)] {
            let mut malformed = valid.clone();
            malformed[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
            assert!(decode_closed_recovery_evidence_v2(&malformed).is_err());
        }
        let mut impossible_resume = valid;
        impossible_resume[presence_offset + 2..presence_offset + 10]
            .copy_from_slice(&6_i64.to_be_bytes());
        assert!(decode_closed_recovery_evidence_v2(&impossible_resume).is_err());
    }

    #[test]
    fn root_rejects_unknown_optional_tags_format_and_trailing_bytes() {
        let valid = root_frame();
        let request_end = 4 * 8
            + "00112233445566778899aabbccddeeff".len()
            + "tenant:1".len()
            + "installation:1".len()
            + "deployment:1".len()
            + 8
            + 8
            + 32
            + 8
            + 2;
        let convergence_end = request_end + 8;
        let controller_tag_offset = convergence_end;
        let controller_frame_end = controller_tag_offset + 2 + 8 + "controller:1".len();
        let fencing_tag_offset = controller_frame_end;
        let format_offset = fencing_tag_offset + 2 + 8;

        let mut unknown_controller = valid.clone();
        unknown_controller[controller_tag_offset..controller_tag_offset + 2]
            .copy_from_slice(&2_i16.to_be_bytes());
        assert!(decode_root(&unknown_controller).is_err());

        let mut unknown_fencing = valid.clone();
        unknown_fencing[fencing_tag_offset..fencing_tag_offset + 2]
            .copy_from_slice(&2_i16.to_be_bytes());
        assert!(decode_root(&unknown_fencing).is_err());

        let mut unknown_format = valid.clone();
        unknown_format[format_offset..format_offset + 2].copy_from_slice(&2_i16.to_be_bytes());
        assert!(decode_root(&unknown_format).is_err());

        let mut trailing = valid;
        trailing.push(0);
        assert!(decode_root(&trailing).is_err());
    }
}
