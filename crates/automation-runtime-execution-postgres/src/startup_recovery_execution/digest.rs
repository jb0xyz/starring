use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::RuntimeExecutionPersistenceErrorV1;

const RECORD_FORMAT_VERSION: i16 = 2;
const POSTGRES_EPOCH_UNIX_MICROSECONDS: i64 = 946_684_800_000_000;
const ACTION_PROOF_DOMAIN: &[u8] = b"starring.runtime.startup_recovery.action_proof.v2\0";
const CERTIFICATION_TERMINAL_DOMAIN: &[u8] = b"starring.runtime.certification.terminal.v2\0";
const RESERVED_RESET_RECEIPT_DOMAIN: &[u8] =
    b"starring.runtime.certification.awaiting_reset.receipt.v2";

pub(super) struct RuntimeStartupRecoveryActionProofV2<'a> {
    pub recovery_id: &'a str,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
    pub recovery_class: &'a str,
    pub gateway_shard_id: &'a str,
    pub owner_process_instance_id: &'a str,
    pub owner_lease_epoch: i64,
    pub owner_runtime_build_revision: &'a str,
    pub owner_revision: i64,
    pub owner_expires_at: DateTime<Utc>,
    pub minimum_database_now: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
    pub terminal_projection_bytes: &'a [u8],
}

pub(super) struct RuntimeCertificationTerminalProofV2<'a> {
    pub operation_id: &'a str,
    pub intent_fingerprint: &'a str,
    pub tenant_id: &'a str,
    pub installation_id: &'a str,
    pub deployment_id: &'a str,
    pub deployment_revision: i64,
    pub convergence_attempt: i64,
    pub terminal_outcome_name: &'a str,
    pub resulting_phase: &'a str,
    pub resulting_deployment_revision: i64,
    pub resulting_convergence_attempt: i64,
    pub terminal_at: DateTime<Utc>,
    pub terminal_receipt_bytes: &'a [u8],
}

pub(super) struct RuntimeReservedResetReceiptProofV2<'a> {
    pub recovery_id: &'a str,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub selection_authority_revision: i64,
    pub source_deployment_frame: &'a [u8],
    pub successor_deployment_frame: &'a [u8],
    pub source_slot_frame: &'a [u8],
    pub successor_slot_frame: &'a [u8],
    pub reservation_frame: &'a [u8],
    pub terminal_at: DateTime<Utc>,
}

pub(super) fn startup_recovery_action_digest_v2(
    proof: &RuntimeStartupRecoveryActionProofV2<'_>,
) -> Result<[u8; 32], RuntimeExecutionPersistenceErrorV1> {
    let mut payload = Vec::with_capacity(
        proof
            .terminal_projection_bytes
            .len()
            .checked_add(256)
            .ok_or_else(invalid)?,
    );
    payload.extend_from_slice(&RECORD_FORMAT_VERSION.to_be_bytes());
    push_bytes(&mut payload, proof.recovery_id.as_bytes())?;
    push_i64(&mut payload, proof.originating_emergency_generation);
    push_i64(&mut payload, proof.coordinator_generation);
    push_i64(&mut payload, proof.action_authority_revision);
    push_i64(&mut payload, proof.selection_authority_revision);
    push_bytes(&mut payload, proof.recovery_class.as_bytes())?;
    push_bytes(&mut payload, proof.gateway_shard_id.as_bytes())?;
    push_bytes(&mut payload, proof.owner_process_instance_id.as_bytes())?;
    push_i64(&mut payload, proof.owner_lease_epoch);
    push_bytes(&mut payload, proof.owner_runtime_build_revision.as_bytes())?;
    push_i64(&mut payload, proof.owner_revision);
    push_timestamp(&mut payload, proof.owner_expires_at)?;
    push_timestamp(&mut payload, proof.minimum_database_now)?;
    push_timestamp(&mut payload, proof.recorded_at)?;
    push_bytes(&mut payload, proof.terminal_projection_bytes)?;

    let mut framed = Vec::with_capacity(
        ACTION_PROOF_DOMAIN
            .len()
            .checked_add(payload.len())
            .and_then(|length| length.checked_add(16))
            .ok_or_else(invalid)?,
    );
    push_bytes(&mut framed, ACTION_PROOF_DOMAIN)?;
    push_bytes(&mut framed, &payload)?;
    Ok(Sha256::digest(framed).into())
}

pub(super) fn certification_terminal_digest_v2(
    proof: &RuntimeCertificationTerminalProofV2<'_>,
) -> Result<[u8; 32], RuntimeExecutionPersistenceErrorV1> {
    let mut payload = Vec::with_capacity(
        proof
            .terminal_receipt_bytes
            .len()
            .checked_add(512)
            .ok_or_else(invalid)?,
    );
    payload.extend_from_slice(&RECORD_FORMAT_VERSION.to_be_bytes());
    push_bytes(&mut payload, proof.operation_id.as_bytes())?;
    push_bytes(&mut payload, proof.intent_fingerprint.as_bytes())?;
    push_bytes(&mut payload, proof.tenant_id.as_bytes())?;
    push_bytes(&mut payload, proof.installation_id.as_bytes())?;
    push_bytes(&mut payload, proof.deployment_id.as_bytes())?;
    push_i64(&mut payload, proof.deployment_revision);
    push_i64(&mut payload, proof.convergence_attempt);
    push_bytes(&mut payload, proof.terminal_outcome_name.as_bytes())?;
    push_bytes(&mut payload, proof.resulting_phase.as_bytes())?;
    push_i64(&mut payload, proof.resulting_deployment_revision);
    push_i64(&mut payload, proof.resulting_convergence_attempt);
    push_timestamp(&mut payload, proof.terminal_at)?;
    push_bytes(&mut payload, proof.terminal_receipt_bytes)?;
    let mut framed = Vec::with_capacity(
        CERTIFICATION_TERMINAL_DOMAIN
            .len()
            .checked_add(payload.len())
            .and_then(|length| length.checked_add(16))
            .ok_or_else(invalid)?,
    );
    push_bytes(&mut framed, CERTIFICATION_TERMINAL_DOMAIN)?;
    push_bytes(&mut framed, &payload)?;
    Ok(Sha256::digest(framed).into())
}

pub(super) fn reserved_reset_receipt_bytes_v2(
    proof: &RuntimeReservedResetReceiptProofV2<'_>,
) -> Result<Vec<u8>, RuntimeExecutionPersistenceErrorV1> {
    let mut receipt = Vec::with_capacity(384);
    push_bytes(&mut receipt, RESERVED_RESET_RECEIPT_DOMAIN)?;
    receipt.extend_from_slice(&RECORD_FORMAT_VERSION.to_be_bytes());
    receipt.extend_from_slice(&1_i16.to_be_bytes());
    push_bytes(&mut receipt, proof.recovery_id.as_bytes())?;
    push_i64(&mut receipt, proof.originating_emergency_generation);
    push_i64(&mut receipt, proof.coordinator_generation);
    push_i64(&mut receipt, proof.action_authority_revision);
    push_i64(&mut receipt, proof.selection_authority_revision);
    for frame in [
        proof.source_deployment_frame,
        proof.successor_deployment_frame,
        proof.source_slot_frame,
        proof.successor_slot_frame,
        proof.reservation_frame,
    ] {
        receipt.extend_from_slice(&Sha256::digest(frame));
    }
    push_timestamp(&mut receipt, proof.terminal_at)?;
    Ok(receipt)
}

pub(super) fn lowercase_sha256_bytes(
    value: &str,
) -> Result<[u8; 32], RuntimeExecutionPersistenceErrorV1> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    let encoded = value.as_bytes();
    let mut decoded = [0_u8; 32];
    for (index, pair) in encoded.chunks_exact(2).enumerate() {
        decoded[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Ok(decoded)
}

fn push_bytes(
    target: &mut Vec<u8>,
    value: &[u8],
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let length = i64::try_from(value.len()).map_err(|_| invalid())?;
    push_i64(target, length);
    target.extend_from_slice(value);
    Ok(())
}

fn push_i64(target: &mut Vec<u8>, value: i64) {
    target.extend_from_slice(&value.to_be_bytes());
}

fn push_timestamp(
    target: &mut Vec<u8>,
    value: DateTime<Utc>,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let postgres_microseconds = value
        .timestamp_micros()
        .checked_sub(POSTGRES_EPOCH_UNIX_MICROSECONDS)
        .ok_or_else(invalid)?;
    push_i64(target, postgres_microseconds);
    Ok(())
}

fn hex_digit(value: u8) -> Result<u8, RuntimeExecutionPersistenceErrorV1> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(invalid()),
    }
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn proof<'a>(projection: &'a [u8]) -> RuntimeStartupRecoveryActionProofV2<'a> {
        RuntimeStartupRecoveryActionProofV2 {
            recovery_id: "0123456789abcdef0123456789abcdef",
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
            recovery_class: "stale_live",
            gateway_shard_id: "shard:0",
            owner_process_instance_id: "process:1",
            owner_lease_epoch: 6,
            owner_runtime_build_revision: "build:1",
            owner_revision: 7,
            owner_expires_at: at("2026-01-02T03:04:05.123456Z"),
            minimum_database_now: at("2026-01-02T03:00:00.000001Z"),
            recorded_at: at("2026-01-02T03:01:02.000003Z"),
            terminal_projection_bytes: projection,
        }
    }

    #[test]
    fn digest_matches_the_postgresql_16_golden_vector() {
        let digest = startup_recovery_action_digest_v2(&proof(&[1, 2, 3, 4, 255])).unwrap();
        assert_eq!(
            digest,
            lowercase_sha256_bytes(
                "e2b4bf9d3107e84580a820ef8d70c49bd8fc89affa400166daec35b661f04319"
            )
            .unwrap()
        );
    }

    #[test]
    fn digest_changes_for_every_terminal_and_causality_field() {
        let original = startup_recovery_action_digest_v2(&proof(&[1, 2, 3])).unwrap();
        let changed_projection = startup_recovery_action_digest_v2(&proof(&[1, 2, 4])).unwrap();
        let mut changed_identity = proof(&[1, 2, 3]);
        changed_identity.action_authority_revision += 1;
        let changed_identity = startup_recovery_action_digest_v2(&changed_identity).unwrap();
        assert_ne!(original, changed_projection);
        assert_ne!(original, changed_identity);
    }

    #[test]
    fn digest_text_is_strict_lowercase_sha256() {
        assert!(lowercase_sha256_bytes(&"0".repeat(64)).is_ok());
        for invalid_digest in [
            "0".repeat(63),
            "0".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            assert!(lowercase_sha256_bytes(&invalid_digest).is_err());
        }
    }

    #[test]
    fn reserved_receipt_binds_every_raw_projection_frame() {
        let proof = RuntimeReservedResetReceiptProofV2 {
            recovery_id: "0123456789abcdef0123456789abcdef",
            originating_emergency_generation: 2,
            coordinator_generation: 3,
            action_authority_revision: 5,
            selection_authority_revision: 4,
            source_deployment_frame: b"source",
            successor_deployment_frame: b"successor",
            source_slot_frame: b"slot-source",
            successor_slot_frame: b"slot-successor",
            reservation_frame: b"reservation",
            terminal_at: at("2026-01-02T03:01:02.000003Z"),
        };
        let original = reserved_reset_receipt_bytes_v2(&proof).unwrap();
        let changed = reserved_reset_receipt_bytes_v2(&RuntimeReservedResetReceiptProofV2 {
            source_deployment_frame: b"forged",
            ..proof
        })
        .unwrap();
        assert_ne!(original, changed);
    }

    #[test]
    fn certification_terminal_digest_binds_receipt_and_terminal_scalar() {
        let proof = RuntimeCertificationTerminalProofV2 {
            operation_id: "00112233445566778899aabbccddeeff",
            intent_fingerprint: &"a".repeat(64),
            tenant_id: "tenant:1",
            installation_id: "installation:1",
            deployment_id: "deployment:1",
            deployment_revision: 8,
            convergence_attempt: 5,
            terminal_outcome_name: "awaiting_reset",
            resulting_phase: "reconciling_panels",
            resulting_deployment_revision: 9,
            resulting_convergence_attempt: 5,
            terminal_at: at("2026-01-02T03:01:02.000003Z"),
            terminal_receipt_bytes: b"receipt",
        };
        let original = certification_terminal_digest_v2(&proof).unwrap();
        let changed = certification_terminal_digest_v2(&RuntimeCertificationTerminalProofV2 {
            terminal_receipt_bytes: b"changed",
            ..proof
        })
        .unwrap();
        assert_ne!(original, changed);
    }
}
