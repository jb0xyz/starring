use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeIngressOpenAcknowledgementInputV2,
    RuntimeIngressOpenAcknowledgementReceiptInputV2, RuntimeIngressOpenAcknowledgementReceiptV2,
    RuntimeIngressOpenAcknowledgementRequestDigestV2, RuntimeIngressOpenAcknowledgementV2,
    RuntimeObservedIngressOpenAcknowledgementV2, RuntimePublishIngressOpenAcknowledgementOutcomeV2,
    RuntimePublishIngressOpenAcknowledgementV2, RuntimeUnixMicrosecondsV2,
    RuntimeWriterFenceGenerationV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, TimeDelta, Utc};
use sha2::{Digest, Sha256};

use crate::RuntimeExecutionPersistenceErrorV1;

const MIN_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE: usize = 197;
const MAX_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE: usize = 578;
const MIN_CANONICAL_REQUEST_BYTES_WITH_SOURCE: usize = 205;
const MAX_CANONICAL_REQUEST_BYTES_WITH_SOURCE: usize = 586;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeIngressOpenAcknowledgementOperationRowV2 {
    outcome_name: String,
    gateway_shard_id: String,
    source_acknowledgement_revision: Option<i64>,
    request_digest: Option<Vec<u8>>,
    canonical_request_bytes: Option<Vec<u8>>,
    fence_generation: Option<i64>,
    maintenance_gate_generation: Option<i64>,
    process_instance_id: Option<String>,
    owner_lease_epoch: Option<i64>,
    expected_build_revision: Option<String>,
    observed_owner_revision: Option<i64>,
    requested_owner_observed_at: Option<DateTime<Utc>>,
    requested_owner_expires_at: Option<DateTime<Utc>>,
    connection_epoch: Option<i64>,
    admission_revision: Option<i64>,
    connected_event_sequence: Option<i64>,
    resume_sequence: Option<i64>,
    acknowledgement_revision: Option<i64>,
    acknowledged_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeIngressOpenAcknowledgementOperationRowV2 {
    pub(crate) fn decode_observation(
        self,
    ) -> Result<RuntimeObservedIngressOpenAcknowledgementV2, RuntimeExecutionPersistenceErrorV1>
    {
        match self.outcome_name.as_str() {
            "missing" => self.decode_missing(),
            "present" => self.decode_present(),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn decode_publish(
        self,
        request: &RuntimePublishIngressOpenAcknowledgementV2,
    ) -> Result<RuntimePublishIngressOpenAcknowledgementOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        match self.outcome_name.as_str() {
            "applied" | "replayed" => {
                let applied = self.outcome_name == "applied";
                if self.canonical_request_bytes.as_deref()
                    != Some(request.canonical_request_bytes())
                    || self.request_digest.as_deref() != Some(request.request_digest().as_bytes())
                {
                    return Err(invalid());
                }
                let receipt = self.decode_receipt()?;
                if applied {
                    Ok(RuntimePublishIngressOpenAcknowledgementOutcomeV2::Applied(
                        receipt,
                    ))
                } else {
                    Ok(RuntimePublishIngressOpenAcknowledgementOutcomeV2::Replayed(
                        receipt,
                    ))
                }
            }
            "not_current" => self
                .decode_optional_observation()
                .map(RuntimePublishIngressOpenAcknowledgementOutcomeV2::NotCurrent),
            _ => Err(invalid()),
        }
    }

    fn decode_optional_observation(
        self,
    ) -> Result<RuntimeObservedIngressOpenAcknowledgementV2, RuntimeExecutionPersistenceErrorV1>
    {
        if self.all_persisted_fields_absent() {
            self.decode_missing()
        } else {
            self.decode_present()
        }
    }

    fn decode_missing(
        self,
    ) -> Result<RuntimeObservedIngressOpenAcknowledgementV2, RuntimeExecutionPersistenceErrorV1>
    {
        if !self.all_persisted_fields_absent() {
            return Err(invalid());
        }
        RuntimeObservedIngressOpenAcknowledgementV2::missing(
            parse_shard(&self.gateway_shard_id)?,
            self.observed_database_now,
        )
        .map_err(|_| invalid())
    }

    fn decode_present(
        self,
    ) -> Result<RuntimeObservedIngressOpenAcknowledgementV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.decode_receipt()
            .map(RuntimeObservedIngressOpenAcknowledgementV2::present)
    }

    fn decode_receipt(
        self,
    ) -> Result<RuntimeIngressOpenAcknowledgementReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        let source_acknowledgement_revision =
            optional_positive(self.source_acknowledgement_revision)?;
        let request_digest = digest(self.request_digest.as_deref().ok_or_else(invalid)?)?;
        let canonical_request_bytes = self
            .canonical_request_bytes
            .as_deref()
            .ok_or_else(invalid)?;
        let (minimum_request_bytes, maximum_request_bytes) =
            canonical_request_bounds(source_acknowledgement_revision);
        let computed_digest: [u8; 32] = Sha256::digest(canonical_request_bytes).into();
        if canonical_request_bytes.len() < minimum_request_bytes
            || canonical_request_bytes.len() > maximum_request_bytes
            || computed_digest != *request_digest.as_bytes()
        {
            return Err(invalid());
        }
        let process_instance_id =
            ProcessInstanceId::parse(self.process_instance_id.as_deref().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        let owner_observed_at = self.requested_owner_observed_at.ok_or_else(invalid)?;
        let owner_expires_at = self.requested_owner_expires_at.ok_or_else(invalid)?;
        let acknowledged_at = self.acknowledged_at.ok_or_else(invalid)?;
        let expires_at = self.expires_at.ok_or_else(invalid)?;
        if RuntimeUnixMicrosecondsV2::from_datetime(owner_observed_at).is_err()
            || RuntimeUnixMicrosecondsV2::from_datetime(owner_expires_at).is_err()
            || owner_observed_at >= owner_expires_at
            || acknowledged_at < owner_observed_at
            || expires_at > owner_expires_at
            || expires_at.signed_duration_since(acknowledged_at) > TimeDelta::seconds(10)
        {
            return Err(invalid());
        }
        let acknowledgement =
            RuntimeIngressOpenAcknowledgementV2::new(RuntimeIngressOpenAcknowledgementInputV2 {
                fence_generation: RuntimeWriterFenceGenerationV1::new(positive(
                    self.fence_generation,
                )?),
                maintenance_gate_generation: positive(self.maintenance_gate_generation)?,
                gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                    gateway_shard_id: parse_shard(&self.gateway_shard_id)?,
                    process_instance_id: process_instance_id.clone(),
                    lease_epoch: positive(self.owner_lease_epoch)?,
                    expected_build_revision: RuntimeBuildRevisionV1::parse(
                        self.expected_build_revision
                            .as_deref()
                            .ok_or_else(invalid)?,
                    )
                    .map_err(|_| invalid())?,
                },
                observed_owner_revision: positive(self.observed_owner_revision)?,
                process_instance_id,
                connection_epoch: positive(self.connection_epoch)?,
                admission_revision: positive(self.admission_revision)?,
                connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(positive(
                    self.connected_event_sequence,
                )?),
                resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(positive(
                    self.resume_sequence,
                )?),
                acknowledgement_revision: positive(self.acknowledgement_revision)?,
                acknowledged_at,
                expires_at,
            })
            .map_err(|_| invalid())?;
        RuntimeIngressOpenAcknowledgementReceiptV2::new(
            RuntimeIngressOpenAcknowledgementReceiptInputV2 {
                source_acknowledgement_revision,
                request_digest,
                acknowledgement,
                observed_database_now: self.observed_database_now,
            },
        )
        .map_err(|_| invalid())
    }

    fn all_persisted_fields_absent(&self) -> bool {
        self.source_acknowledgement_revision.is_none()
            && self.request_digest.is_none()
            && self.canonical_request_bytes.is_none()
            && self.fence_generation.is_none()
            && self.maintenance_gate_generation.is_none()
            && self.process_instance_id.is_none()
            && self.owner_lease_epoch.is_none()
            && self.expected_build_revision.is_none()
            && self.observed_owner_revision.is_none()
            && self.requested_owner_observed_at.is_none()
            && self.requested_owner_expires_at.is_none()
            && self.connection_epoch.is_none()
            && self.admission_revision.is_none()
            && self.connected_event_sequence.is_none()
            && self.resume_sequence.is_none()
            && self.acknowledgement_revision.is_none()
            && self.acknowledged_at.is_none()
            && self.expires_at.is_none()
    }
}

fn canonical_request_bounds(source: Option<NonZeroU64>) -> (usize, usize) {
    if source.is_some() {
        (
            MIN_CANONICAL_REQUEST_BYTES_WITH_SOURCE,
            MAX_CANONICAL_REQUEST_BYTES_WITH_SOURCE,
        )
    } else {
        (
            MIN_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE,
            MAX_CANONICAL_REQUEST_BYTES_WITHOUT_SOURCE,
        )
    }
}

fn digest(
    value: &[u8],
) -> Result<RuntimeIngressOpenAcknowledgementRequestDigestV2, RuntimeExecutionPersistenceErrorV1> {
    let bytes: [u8; 32] = value.try_into().map_err(|_| invalid())?;
    Ok(RuntimeIngressOpenAcknowledgementRequestDigestV2::from_bytes(bytes))
}

fn parse_shard(value: &str) -> Result<GatewayShardIdV1, RuntimeExecutionPersistenceErrorV1> {
    GatewayShardIdV1::parse(value).map_err(|_| invalid())
}

fn positive(value: Option<i64>) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    let value = value.ok_or_else(invalid)?;
    let value = u64::try_from(value).map_err(|_| invalid())?;
    NonZeroU64::new(value).ok_or_else(invalid)
}

fn optional_positive(
    value: Option<i64>,
) -> Result<Option<NonZeroU64>, RuntimeExecutionPersistenceErrorV1> {
    value.map(|value| positive(Some(value))).transpose()
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn row(outcome_name: &str) -> RuntimeIngressOpenAcknowledgementOperationRowV2 {
        let canonical_request_bytes = vec![b'x'; MIN_CANONICAL_REQUEST_BYTES_WITH_SOURCE];
        RuntimeIngressOpenAcknowledgementOperationRowV2 {
            outcome_name: outcome_name.to_owned(),
            gateway_shard_id: "shard:0".to_owned(),
            source_acknowledgement_revision: Some(4),
            request_digest: Some(Sha256::digest(&canonical_request_bytes).to_vec()),
            canonical_request_bytes: Some(canonical_request_bytes),
            fence_generation: Some(7),
            maintenance_gate_generation: Some(9),
            process_instance_id: Some("process:1".to_owned()),
            owner_lease_epoch: Some(11),
            expected_build_revision: Some("build:1".to_owned()),
            observed_owner_revision: Some(13),
            requested_owner_observed_at: Some(at(100)),
            requested_owner_expires_at: Some(at(130)),
            connection_epoch: Some(15),
            admission_revision: Some(17),
            connected_event_sequence: Some(19),
            resume_sequence: Some(21),
            acknowledgement_revision: Some(5),
            acknowledged_at: Some(at(101)),
            expires_at: Some(at(106)),
            observed_database_now: at(102),
        }
    }

    #[test]
    fn present_and_expired_rows_remain_exactly_decodable() {
        let present = row("present").decode_observation().unwrap();
        assert!(matches!(
            present,
            RuntimeObservedIngressOpenAcknowledgementV2::Present(_)
        ));
        let mut expired = row("present");
        expired.observed_database_now = at(107);
        assert!(matches!(
            expired.decode_observation().unwrap(),
            RuntimeObservedIngressOpenAcknowledgementV2::Present(_)
        ));
    }

    #[test]
    fn missing_rows_reject_mixed_persisted_fields() {
        let mut missing = row("missing");
        missing.source_acknowledgement_revision = None;
        missing.request_digest = None;
        missing.canonical_request_bytes = None;
        missing.fence_generation = None;
        missing.maintenance_gate_generation = None;
        missing.process_instance_id = None;
        missing.owner_lease_epoch = None;
        missing.expected_build_revision = None;
        missing.observed_owner_revision = None;
        missing.requested_owner_observed_at = None;
        missing.requested_owner_expires_at = None;
        missing.connection_epoch = None;
        missing.admission_revision = None;
        missing.connected_event_sequence = None;
        missing.resume_sequence = None;
        missing.acknowledgement_revision = None;
        missing.acknowledged_at = None;
        missing.expires_at = None;
        assert!(matches!(
            missing.clone().decode_observation().unwrap(),
            RuntimeObservedIngressOpenAcknowledgementV2::Missing { .. }
        ));
        missing.resume_sequence = Some(21);
        assert_eq!(missing.decode_observation(), Err(invalid()));
    }

    #[test]
    fn persisted_digest_identity_and_intervals_fail_closed() {
        let mut bad_digest = row("present");
        bad_digest.request_digest = Some(vec![0; 32]);
        assert_eq!(bad_digest.decode_observation(), Err(invalid()));
        let mut bad_process = row("present");
        bad_process.process_instance_id = Some(String::new());
        assert_eq!(bad_process.decode_observation(), Err(invalid()));
        let mut bad_resume = row("present");
        bad_resume.resume_sequence = Some(19);
        assert_eq!(bad_resume.decode_observation(), Err(invalid()));
        let mut bad_revision = row("present");
        bad_revision.acknowledgement_revision = Some(6);
        assert_eq!(bad_revision.decode_observation(), Err(invalid()));
    }

    #[test]
    fn persisted_canonical_bounds_and_owner_envelope_fail_closed() {
        let mut short_request = row("present");
        short_request.canonical_request_bytes = Some(vec![b'x'; 204]);
        short_request.request_digest =
            Some(Sha256::digest(short_request.canonical_request_bytes.as_ref().unwrap()).to_vec());
        assert_eq!(short_request.decode_observation(), Err(invalid()));

        let mut noncanonical_owner_time = row("present");
        noncanonical_owner_time.requested_owner_observed_at = Some(at(-62_135_596_801));
        assert_eq!(noncanonical_owner_time.decode_observation(), Err(invalid()));

        let mut before_owner_observation = row("present");
        before_owner_observation.acknowledged_at = Some(at(99));
        assert_eq!(
            before_owner_observation.decode_observation(),
            Err(invalid())
        );

        let mut exceeds_owner = row("present");
        exceeds_owner.requested_owner_expires_at = Some(at(105));
        assert_eq!(exceeds_owner.decode_observation(), Err(invalid()));

        let mut exceeds_ten_seconds = row("present");
        exceeds_ten_seconds.expires_at = Some(at(112));
        assert_eq!(exceeds_ten_seconds.decode_observation(), Err(invalid()));
    }
}
