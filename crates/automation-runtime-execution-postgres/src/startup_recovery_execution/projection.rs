use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::RuntimeExecutionPersistenceErrorV1;

const TERMINAL_PROJECTION_DOMAIN: &[u8] =
    b"starring.runtime.startup_recovery.stale_live.terminal.v2";
const TERMINAL_PROJECTION_VERSION: i16 = 2;
const NO_CANDIDATE_TAG: i16 = 0;
const PROGRESSED_TAG: i16 = 1;
const PROGRESSED_JSONB_FIELD_COUNT: usize = 5;
const PROGRESSED_TRAILER_LENGTH: usize = 18;
pub(super) const MAX_TERMINAL_PROJECTION_BYTES: usize = 1_048_576;

pub(super) enum RuntimeStartupRecoveryTerminalProjectionV2 {
    NoCandidate,
    Progressed(Box<RuntimeStartupRecoveryProgressedProjectionV2>),
}

pub(super) struct RuntimeStartupRecoveryProgressedProjectionV2 {
    pub previous_deployment: Value,
    pub terminal_deployment: Value,
    pub previous_slot_fence: Value,
    pub terminal_slot_fence: Value,
    pub serving_lease: Value,
    pub recovery_kind: i16,
    pub evidence_at: DateTime<Utc>,
    pub recovered_at: DateTime<Utc>,
}

pub(super) fn decode_terminal_projection_v2(
    terminal_outcome_name: &str,
    projection: &[u8],
) -> Result<RuntimeStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1> {
    if projection.is_empty() || projection.len() > MAX_TERMINAL_PROJECTION_BYTES {
        return Err(invalid());
    }
    match terminal_outcome_name {
        "no_candidate" => {
            if projection == projection_prefix(NO_CANDIDATE_TAG) {
                Ok(RuntimeStartupRecoveryTerminalProjectionV2::NoCandidate)
            } else {
                Err(invalid())
            }
        }
        "progressed" => decode_progressed_projection(projection),
        _ => Err(invalid()),
    }
}

fn decode_progressed_projection(
    projection: &[u8],
) -> Result<RuntimeStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1> {
    let prefix = projection_prefix(PROGRESSED_TAG);
    let Some(mut remainder) = projection.strip_prefix(prefix.as_slice()) else {
        return Err(invalid());
    };
    let mut rows = Vec::with_capacity(PROGRESSED_JSONB_FIELD_COUNT);
    for _ in 0..PROGRESSED_JSONB_FIELD_COUNT {
        let (length_bytes, following) = take(remainder, 8)?;
        let length = i64::from_be_bytes(length_bytes.try_into().map_err(|_| invalid())?);
        let length = usize::try_from(length).map_err(|_| invalid())?;
        let (jsonb, following) = take(following, length)?;
        if jsonb.first() != Some(&1) {
            return Err(invalid());
        }
        let row = serde_json::from_slice::<Value>(&jsonb[1..]).map_err(|_| invalid())?;
        if !row.is_object() {
            return Err(invalid());
        }
        rows.push(row);
        remainder = following;
    }
    if remainder.len() != PROGRESSED_TRAILER_LENGTH {
        return Err(invalid());
    }
    let recovery_kind = i16::from_be_bytes(remainder[..2].try_into().map_err(|_| invalid())?);
    if !matches!(recovery_kind, 1 | 2) {
        return Err(invalid());
    }
    let evidence = i64::from_be_bytes(remainder[2..10].try_into().map_err(|_| invalid())?);
    let mutation_clock = i64::from_be_bytes(remainder[10..18].try_into().map_err(|_| invalid())?);
    if matches!(evidence, i64::MIN | i64::MAX)
        || matches!(mutation_clock, i64::MIN | i64::MAX)
        || evidence > mutation_clock
    {
        return Err(invalid());
    }
    let mut rows = rows.into_iter();
    Ok(RuntimeStartupRecoveryTerminalProjectionV2::Progressed(
        Box::new(RuntimeStartupRecoveryProgressedProjectionV2 {
            previous_deployment: rows.next().ok_or_else(invalid)?,
            terminal_deployment: rows.next().ok_or_else(invalid)?,
            previous_slot_fence: rows.next().ok_or_else(invalid)?,
            terminal_slot_fence: rows.next().ok_or_else(invalid)?,
            serving_lease: rows.next().ok_or_else(invalid)?,
            recovery_kind,
            evidence_at: postgres_timestamp(evidence)?,
            recovered_at: postgres_timestamp(mutation_clock)?,
        }),
    ))
}

fn postgres_timestamp(
    postgres_microseconds: i64,
) -> Result<DateTime<Utc>, RuntimeExecutionPersistenceErrorV1> {
    let unix_microseconds = postgres_microseconds
        .checked_add(946_684_800_000_000)
        .ok_or_else(invalid)?;
    DateTime::from_timestamp_micros(unix_microseconds).ok_or_else(invalid)
}

fn projection_prefix(outcome: i16) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(TERMINAL_PROJECTION_DOMAIN.len() + 12);
    prefix.extend_from_slice(
        &i64::try_from(TERMINAL_PROJECTION_DOMAIN.len())
            .expect("terminal projection domain length fits i64")
            .to_be_bytes(),
    );
    prefix.extend_from_slice(TERMINAL_PROJECTION_DOMAIN);
    prefix.extend_from_slice(&TERMINAL_PROJECTION_VERSION.to_be_bytes());
    prefix.extend_from_slice(&outcome.to_be_bytes());
    prefix
}

fn take(value: &[u8], length: usize) -> Result<(&[u8], &[u8]), RuntimeExecutionPersistenceErrorV1> {
    if value.len() < length {
        return Err(invalid());
    }
    Ok(value.split_at(length))
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progressed_projection() -> Vec<u8> {
        let mut projection = projection_prefix(PROGRESSED_TAG);
        for index in 0..PROGRESSED_JSONB_FIELD_COUNT {
            let jsonb = [vec![1], format!("{{\"field\": {index}}}").into_bytes()].concat();
            projection.extend_from_slice(&(jsonb.len() as i64).to_be_bytes());
            projection.extend_from_slice(&jsonb);
        }
        projection.extend_from_slice(&1_i16.to_be_bytes());
        projection.extend_from_slice(&10_i64.to_be_bytes());
        projection.extend_from_slice(&11_i64.to_be_bytes());
        projection
    }

    #[test]
    fn no_candidate_is_an_exact_minimal_projection() {
        let projection = projection_prefix(NO_CANDIDATE_TAG);
        assert!(matches!(
            decode_terminal_projection_v2("no_candidate", &projection).unwrap(),
            RuntimeStartupRecoveryTerminalProjectionV2::NoCandidate
        ));
        let mut extended = projection;
        extended.push(0);
        assert!(decode_terminal_projection_v2("no_candidate", &extended).is_err());
    }

    #[test]
    fn progressed_requires_all_framed_canonical_rows_and_exact_trailer() {
        let projection = progressed_projection();
        assert!(matches!(
            decode_terminal_projection_v2("progressed", &projection).unwrap(),
            RuntimeStartupRecoveryTerminalProjectionV2::Progressed(_)
        ));
        for truncated_length in [
            1,
            projection_prefix(PROGRESSED_TAG).len() + 7,
            projection.len() - 1,
        ] {
            assert!(
                decode_terminal_projection_v2("progressed", &projection[..truncated_length])
                    .is_err()
            );
        }
        let mut trailing = projection;
        trailing.push(0);
        assert!(decode_terminal_projection_v2("progressed", &trailing).is_err());
    }

    #[test]
    fn outcome_name_must_match_the_projection_tag() {
        assert!(
            decode_terminal_projection_v2("progressed", &projection_prefix(NO_CANDIDATE_TAG))
                .is_err()
        );
        assert!(decode_terminal_projection_v2("no_candidate", &progressed_projection()).is_err());
        assert!(decode_terminal_projection_v2("retry_after", &[]).is_err());
    }
}
