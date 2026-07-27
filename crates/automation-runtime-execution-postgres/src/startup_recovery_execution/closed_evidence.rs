use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeCanonicalRouteMutationProvenanceV2, RuntimeClosedRecoveryRouteWitnessV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeRouteMutationProvenanceV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use crate::RuntimeExecutionPersistenceErrorV1;

const READY_KIND_TAG: i16 = 1;
const RESUMED_KIND_TAG: i16 = 2;
const ABSENT_TAG: i16 = 0;
const PRESENT_TAG: i16 = 1;

pub(super) struct RuntimeClosedRecoveryEvidenceV2 {
    pub paused_process_instance_id: String,
    pub paused_coordinator_generation: i64,
    pub paused_connection_epoch: i64,
    pub paused_ready_kind: &'static str,
    pub paused_admission_revision: i64,
    pub paused_transition_sequence: i64,
    pub paused_connected_event_sequence: i64,
    pub paused_last_resume_sequence: Option<i64>,
    pub registry_process_instance_id: String,
    pub registry_observation_sequence: i64,
    pub registry_retained_slot_count: i64,
    pub registry_retained_empty_tombstone_count: i64,
}

#[derive(Clone)]
pub(super) struct RuntimeClosedRecoveryExpectedEvidenceV2 {
    pub paused_process_instance_id: String,
    pub paused_coordinator_generation: i64,
    pub paused_connection_epoch: i64,
    pub paused_ready_kind: &'static str,
    pub paused_admission_revision: i64,
    pub paused_transition_sequence: i64,
    pub paused_connected_event_sequence: i64,
    pub paused_last_resume_sequence: Option<i64>,
    pub registry_process_instance_id: String,
    pub registry_observation_sequence: i64,
    pub registry_retained_slot_count: i64,
    pub registry_retained_empty_tombstone_count: i64,
}

pub(super) struct RuntimeClosedRecoveryProvenanceExpectationV2<'a> {
    pub recovery_id: &'a str,
    pub originating_emergency_generation: i64,
    pub coordinator_generation: i64,
    pub action_authority_revision: i64,
    pub gateway_owner_lease_id: &'a RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: i64,
    pub owner_expires_at: DateTime<Utc>,
    pub evidence: &'a RuntimeClosedRecoveryExpectedEvidenceV2,
}

pub(super) fn decode_closed_recovery_evidence_v2(
    frame: &[u8],
) -> Result<RuntimeClosedRecoveryEvidenceV2, RuntimeExecutionPersistenceErrorV1> {
    let mut cursor = Cursor::new(frame);
    let paused_process_instance_id = cursor.take_nonempty_text_frame()?;
    let paused_coordinator_generation = cursor.take_positive_i64()?;
    let paused_connection_epoch = cursor.take_positive_i64()?;
    let paused_ready_kind = match cursor.take_i16()? {
        READY_KIND_TAG => "ready",
        RESUMED_KIND_TAG => "resumed",
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
    let evidence = RuntimeClosedRecoveryEvidenceV2 {
        paused_process_instance_id,
        paused_coordinator_generation,
        paused_connection_epoch,
        paused_ready_kind,
        paused_admission_revision,
        paused_transition_sequence,
        paused_connected_event_sequence,
        paused_last_resume_sequence,
        registry_process_instance_id: cursor.take_nonempty_text_frame()?,
        registry_observation_sequence: cursor.take_positive_i64()?,
        registry_retained_slot_count: cursor.take_nonnegative_i64()?,
        registry_retained_empty_tombstone_count: cursor.take_nonnegative_i64()?,
    };
    if !cursor.is_empty()
        || evidence.paused_transition_sequence < evidence.paused_connected_event_sequence
        || evidence
            .paused_last_resume_sequence
            .is_some_and(|sequence| {
                sequence <= evidence.paused_connected_event_sequence
                    || sequence > evidence.paused_transition_sequence
            })
        || evidence.registry_retained_slot_count != evidence.registry_retained_empty_tombstone_count
    {
        return Err(invalid());
    }
    Ok(evidence)
}

pub(super) fn validate_closed_recovery_evidence_v2(
    actual: &RuntimeClosedRecoveryEvidenceV2,
    expected: &RuntimeClosedRecoveryExpectedEvidenceV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    if actual.paused_process_instance_id != expected.paused_process_instance_id
        || actual.paused_coordinator_generation != expected.paused_coordinator_generation
        || actual.paused_connection_epoch != expected.paused_connection_epoch
        || actual.paused_ready_kind != expected.paused_ready_kind
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
        Err(invalid())
    } else {
        Ok(())
    }
}

pub(super) fn closed_recovery_route_witness_v2(
    expected: &RuntimeClosedRecoveryProvenanceExpectationV2<'_>,
) -> Result<RuntimeClosedRecoveryRouteWitnessV2, RuntimeExecutionPersistenceErrorV1> {
    let evidence = expected.evidence;
    if evidence.paused_process_instance_id
        != expected.gateway_owner_lease_id.process_instance_id.as_str()
        || evidence.registry_process_instance_id != evidence.paused_process_instance_id
        || evidence.paused_coordinator_generation != expected.originating_emergency_generation
    {
        return Err(invalid());
    }
    Ok(RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: automation_runtime_controller::RuntimeRecoveryIdV2::parse(
            expected.recovery_id,
        )
        .map_err(|_| invalid())?,
        originating_emergency_generation: positive_non_zero(
            expected.originating_emergency_generation,
        )?,
        recovery_generation: positive_non_zero(expected.coordinator_generation)?,
        recovery_authority_revision: positive_non_zero(expected.action_authority_revision)?,
        gateway_owner_lease_id: expected.gateway_owner_lease_id.clone(),
        observed_owner_revision: positive_non_zero(expected.owner_revision)?,
        owner_expires_at: expected.owner_expires_at,
        process_instance_id: ProcessInstanceId::parse(&evidence.paused_process_instance_id)
            .map_err(|_| invalid())?,
        connection_epoch: positive_non_zero(evidence.paused_connection_epoch)?,
        paused_admission_revision: positive_non_zero(evidence.paused_admission_revision)?,
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(positive_non_zero(
            evidence.paused_connected_event_sequence,
        )?),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(positive_non_zero(
            evidence.paused_transition_sequence,
        )?),
    })
}

pub(super) fn closed_recovery_provenance_v2(
    expected: &RuntimeClosedRecoveryProvenanceExpectationV2<'_>,
) -> Result<RuntimeCanonicalRouteMutationProvenanceV2, RuntimeExecutionPersistenceErrorV1> {
    RuntimeCanonicalRouteMutationProvenanceV2::new(
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_recovery_route_witness_v2(
            expected,
        )?),
    )
    .map_err(|_| invalid())
}

fn positive_non_zero(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
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

    fn take_nonnegative_i64(&mut self) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
        let value = self.take_i64()?;
        if value < 0 {
            Err(invalid())
        } else {
            Ok(value)
        }
    }

    fn take_nonempty_frame(&mut self) -> Result<&'a [u8], RuntimeExecutionPersistenceErrorV1> {
        let length = usize::try_from(self.take_i64()?).map_err(|_| invalid())?;
        if length == 0 || length > super::projection::MAX_TERMINAL_PROJECTION_BYTES {
            return Err(invalid());
        }
        self.take(length)
    }

    fn take_nonempty_text_frame(&mut self) -> Result<String, RuntimeExecutionPersistenceErrorV1> {
        std::str::from_utf8(self.take_nonempty_frame()?)
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

    fn is_empty(&self) -> bool {
        self.remainder.is_empty()
    }
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}
