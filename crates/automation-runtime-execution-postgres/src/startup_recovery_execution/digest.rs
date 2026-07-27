use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::RuntimeExecutionPersistenceErrorV1;

const RECORD_FORMAT_VERSION: i16 = 2;
const POSTGRES_EPOCH_UNIX_MICROSECONDS: i64 = 946_684_800_000_000;
const ACTION_PROOF_DOMAIN: &[u8] = b"starring.runtime.startup_recovery.action_proof.v2\0";

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
}
