use std::num::NonZeroU32;

use automation_runtime_controller::{
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
    RuntimeDeploymentScopeV1, RuntimeReservedCertificationIntentV2,
};
use automation_runtime_convergence::{DeploymentId, DeploymentRevision, InstallationId, TenantId};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::projection::MAX_TERMINAL_PROJECTION_BYTES;
use crate::RuntimeExecutionPersistenceErrorV1;

const TERMINAL_PROJECTION_DOMAIN: &[u8] =
    b"starring.runtime.startup_recovery.reserved_awaiting_certification.terminal.v2";
const TERMINAL_PROJECTION_VERSION: i16 = 2;
const NO_CANDIDATE_TAG: i16 = 0;
const PROGRESSED_TAG: i16 = 1;
const UNRESERVED_PROGRESSED_TAG: i16 = 2;
const RESERVATION_FRAME_VERSION: i16 = 2;
const TERMINAL_SCALAR_VERSION: i16 = 2;

pub(super) enum RuntimeReservedStartupRecoveryTerminalProjectionV2 {
    NoCandidate,
    Progressed(Box<RuntimeReservedStartupRecoveryProgressedProjectionV2>),
    UnreservedProgressed(Box<RuntimeUnreservedStartupRecoveryProgressedProjectionV2>),
}

pub(super) struct RuntimeReservedStartupRecoveryProgressedProjectionV2 {
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub source_deployment: Value,
    pub source_deployment_frame: Box<[u8]>,
    pub successor_deployment: Value,
    pub successor_deployment_frame: Box<[u8]>,
    pub source_slot_fence: Value,
    pub source_slot_fence_frame: Box<[u8]>,
    pub successor_slot_fence: Value,
    pub successor_slot_fence_frame: Box<[u8]>,
    pub reservation: RuntimeReservedCertificationIntentV2,
    pub reservation_frame: Box<[u8]>,
    pub terminal_outcome_name: String,
    pub resulting_phase: String,
    pub resulting_deployment_revision: DeploymentRevision,
    pub resulting_convergence_attempt: NonZeroU32,
    pub terminal_at: DateTime<Utc>,
    pub terminal_receipt_bytes: Box<[u8]>,
    pub terminal_receipt_digest: String,
}

pub(super) struct RuntimeUnreservedStartupRecoveryProgressedProjectionV2 {
    pub source_deployment: Value,
    pub successor_deployment: Value,
    pub source_slot_fence: Value,
    pub successor_slot_fence: Value,
    pub terminal_at: DateTime<Utc>,
}

pub(super) fn decode_reserved_terminal_projection_v2(
    terminal_outcome_name: &str,
    projection: &[u8],
) -> Result<RuntimeReservedStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1>
{
    if projection.is_empty() || projection.len() > MAX_TERMINAL_PROJECTION_BYTES {
        return Err(invalid());
    }
    match terminal_outcome_name {
        "no_candidate" if projection == projection_prefix(NO_CANDIDATE_TAG) => {
            Ok(RuntimeReservedStartupRecoveryTerminalProjectionV2::NoCandidate)
        }
        "progressed" => decode_progressed_projection(projection),
        _ => Err(invalid()),
    }
}

fn decode_progressed_projection(
    projection: &[u8],
) -> Result<RuntimeReservedStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1>
{
    if projection.starts_with(&projection_prefix(UNRESERVED_PROGRESSED_TAG)) {
        return decode_unreserved_progressed_projection(projection);
    }
    let prefix = projection_prefix(PROGRESSED_TAG);
    let remainder = projection
        .strip_prefix(prefix.as_slice())
        .ok_or_else(invalid)?;
    let mut cursor = Cursor::new(remainder);
    let operation_id = RuntimeCertificationOperationIdV2::parse(cursor.take_fixed_text(32)?)
        .map_err(|_| invalid())?;
    let early_terminal_digest = cursor.take_fixed_text(64)?;
    require_lowercase_digest(&early_terminal_digest)?;
    let source_deployment = cursor.take_jsonb_frame()?;
    let successor_deployment = cursor.take_jsonb_frame()?;
    let source_slot_fence = cursor.take_jsonb_frame()?;
    let successor_slot_fence = cursor.take_jsonb_frame()?;
    let reservation_frame = cursor.take_nonempty_frame()?.to_vec().into_boxed_slice();
    let reservation = decode_reservation_frame(&reservation_frame, &operation_id)?;
    if cursor.take_i16()? != TERMINAL_SCALAR_VERSION {
        return Err(invalid());
    }
    let terminal_outcome_name = cursor.take_nonempty_text_frame()?;
    let resulting_phase = cursor.take_nonempty_text_frame()?;
    if terminal_outcome_name != "awaiting_reset" || resulting_phase != "reconciling_panels" {
        return Err(invalid());
    }
    let resulting_deployment_revision =
        DeploymentRevision::new(positive_u64(cursor.take_i64()?)?).map_err(|_| invalid())?;
    let resulting_convergence_attempt = positive_u32(cursor.take_i64()?)?;
    let terminal_at = postgres_timestamp(cursor.take_i64()?)?;
    let terminal_receipt_bytes = cursor.take_nonempty_frame()?.to_vec().into_boxed_slice();
    let terminal_receipt_digest = cursor.take_nonempty_text_frame()?;
    require_lowercase_digest(&terminal_receipt_digest)?;
    if !cursor.is_empty() || terminal_receipt_digest != early_terminal_digest {
        return Err(invalid());
    }
    Ok(
        RuntimeReservedStartupRecoveryTerminalProjectionV2::Progressed(Box::new(
            RuntimeReservedStartupRecoveryProgressedProjectionV2 {
                operation_id,
                source_deployment: source_deployment.value,
                source_deployment_frame: source_deployment.frame,
                successor_deployment: successor_deployment.value,
                successor_deployment_frame: successor_deployment.frame,
                source_slot_fence: source_slot_fence.value,
                source_slot_fence_frame: source_slot_fence.frame,
                successor_slot_fence: successor_slot_fence.value,
                successor_slot_fence_frame: successor_slot_fence.frame,
                reservation,
                reservation_frame,
                terminal_outcome_name,
                resulting_phase,
                resulting_deployment_revision,
                resulting_convergence_attempt,
                terminal_at,
                terminal_receipt_bytes,
                terminal_receipt_digest,
            },
        )),
    )
}

fn decode_unreserved_progressed_projection(
    projection: &[u8],
) -> Result<RuntimeReservedStartupRecoveryTerminalProjectionV2, RuntimeExecutionPersistenceErrorV1>
{
    let prefix = projection_prefix(UNRESERVED_PROGRESSED_TAG);
    let remainder = projection
        .strip_prefix(prefix.as_slice())
        .ok_or_else(invalid)?;
    let mut cursor = Cursor::new(remainder);
    let source_deployment = cursor.take_jsonb_frame()?;
    let successor_deployment = cursor.take_jsonb_frame()?;
    let source_slot_fence = cursor.take_jsonb_frame()?;
    let successor_slot_fence = cursor.take_jsonb_frame()?;
    let terminal_at = postgres_timestamp(cursor.take_i64()?)?;
    if !cursor.is_empty() {
        return Err(invalid());
    }
    Ok(
        RuntimeReservedStartupRecoveryTerminalProjectionV2::UnreservedProgressed(Box::new(
            RuntimeUnreservedStartupRecoveryProgressedProjectionV2 {
                source_deployment: source_deployment.value,
                successor_deployment: successor_deployment.value,
                source_slot_fence: source_slot_fence.value,
                successor_slot_fence: successor_slot_fence.value,
                terminal_at,
            },
        )),
    )
}

fn decode_reservation_frame(
    frame: &[u8],
    expected_operation_id: &RuntimeCertificationOperationIdV2,
) -> Result<RuntimeReservedCertificationIntentV2, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    if cursor.take_i16()? != RESERVATION_FRAME_VERSION {
        return Err(invalid());
    }
    let operation_id = RuntimeCertificationOperationIdV2::parse(cursor.take_nonempty_text_frame()?)
        .map_err(|_| invalid())?;
    if &operation_id != expected_operation_id {
        return Err(invalid());
    }
    let scope = RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(cursor.take_nonempty_text_frame()?).map_err(|_| invalid())?,
        installation_id: InstallationId::parse(cursor.take_nonempty_text_frame()?)
            .map_err(|_| invalid())?,
        deployment_id: DeploymentId::parse(cursor.take_nonempty_text_frame()?)
            .map_err(|_| invalid())?,
    };
    let deployment_revision =
        DeploymentRevision::new(positive_u64(cursor.take_i64()?)?).map_err(|_| invalid())?;
    let convergence_attempt = positive_u32(cursor.take_i64()?)?;
    let certification_intent_bytes = cursor.take_nonempty_frame()?;
    let fingerprint =
        RuntimeCertificationIntentFingerprintV2::parse(cursor.take_nonempty_text_frame()?)
            .map_err(|_| invalid())?;
    if !cursor.is_empty() {
        return Err(invalid());
    }
    RuntimeReservedCertificationIntentV2::from_persisted(
        scope,
        deployment_revision,
        convergence_attempt,
        &operation_id,
        certification_intent_bytes,
        &fingerprint,
    )
    .map_err(|_| invalid())
}

struct JsonbFrame {
    value: Value,
    frame: Box<[u8]>,
}

struct Cursor<'a> {
    remainder: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(value: &'a [u8]) -> Self {
        Self { remainder: value }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        if self.remainder.len() < length {
            return Err(invalid());
        }
        let (value, remainder) = self.remainder.split_at(length);
        self.remainder = remainder;
        Ok(value)
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

    fn take_nonempty_frame(&mut self) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let length = usize::try_from(self.take_i64()?).map_err(|_| invalid())?;
        if length == 0 || length > MAX_TERMINAL_PROJECTION_BYTES {
            return Err(invalid());
        }
        self.take(length)
    }

    fn take_fixed_text(
        &mut self,
        length: usize,
    ) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
        std::str::from_utf8(self.take(length)?)
            .map(str::to_owned)
            .map_err(|_| invalid())
    }

    fn take_nonempty_text_frame(&mut self) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
        std::str::from_utf8(self.take_nonempty_frame()?)
            .map(str::to_owned)
            .map_err(|_| invalid())
    }

    fn take_jsonb_frame(&mut self) -> Result<JsonbFrame, RuntimeExecutionPersistenceErrorV1> {
        let frame = self.take_nonempty_frame()?;
        if frame.first() != Some(&1) {
            return Err(invalid());
        }
        let value = serde_json::from_slice::<Value>(&frame[1..]).map_err(|_| invalid())?;
        if !value.is_object() {
            return Err(invalid());
        }
        Ok(JsonbFrame {
            value,
            frame: frame.to_vec().into_boxed_slice(),
        })
    }

    fn is_empty(&self) -> bool {
        self.remainder.is_empty()
    }
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

fn require_lowercase_digest(value: &str) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn positive_u64(value: i64) -> Result<u64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}

fn positive_u32(value: i64) -> Result<NonZeroU32, RuntimeExecutionPersistenceErrorV1> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(invalid)
}

fn postgres_timestamp(
    postgres_microseconds: i64,
) -> Result<DateTime<Utc>, RuntimeExecutionPersistenceErrorV1> {
    if matches!(postgres_microseconds, i64::MIN | i64::MAX) {
        return Err(invalid());
    }
    let unix_microseconds = postgres_microseconds
        .checked_add(946_684_800_000_000)
        .ok_or_else(invalid)?;
    DateTime::from_timestamp_micros(unix_microseconds).ok_or_else(invalid)
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_candidate_projection_is_class_specific_and_exact() {
        let projection = projection_prefix(NO_CANDIDATE_TAG);
        assert!(matches!(
            decode_reserved_terminal_projection_v2("no_candidate", &projection).unwrap(),
            RuntimeReservedStartupRecoveryTerminalProjectionV2::NoCandidate
        ));
        let mut stale = projection;
        stale.push(0);
        assert!(decode_reserved_terminal_projection_v2("no_candidate", &stale).is_err());
    }

    #[test]
    fn progressed_projection_rejects_truncated_fixed_identity() {
        let mut projection = projection_prefix(PROGRESSED_TAG);
        projection.extend_from_slice(b"00112233445566778899aabbccddeef");
        assert!(decode_reserved_terminal_projection_v2("progressed", &projection).is_err());
    }

    #[test]
    fn unreserved_projection_requires_four_jsonb_frames_and_terminal_time() {
        let frame = [1, b'{', b'}'];
        let framed = [
            i64::try_from(frame.len()).unwrap().to_be_bytes().as_slice(),
            frame.as_slice(),
        ]
        .concat();
        let mut projection = projection_prefix(UNRESERVED_PROGRESSED_TAG);
        for _ in 0..4 {
            projection.extend_from_slice(&framed);
        }
        projection.extend_from_slice(&0_i64.to_be_bytes());
        assert!(matches!(
            decode_reserved_terminal_projection_v2("progressed", &projection).unwrap(),
            RuntimeReservedStartupRecoveryTerminalProjectionV2::UnreservedProgressed(_)
        ));
        projection.push(0);
        assert!(decode_reserved_terminal_projection_v2("progressed", &projection).is_err());
    }

    #[test]
    fn postgres_timestamp_rejects_sentinels_and_accepts_epoch() {
        assert_eq!(
            postgres_timestamp(0).unwrap(),
            DateTime::from_timestamp(946_684_800, 0).unwrap()
        );
        assert!(postgres_timestamp(i64::MIN).is_err());
        assert!(postgres_timestamp(i64::MAX).is_err());
    }
}
