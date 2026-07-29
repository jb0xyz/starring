use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::v2_canonical_value::RuntimePersistenceU64V2;
use crate::{
    GatewayShardIdV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyAttestationV2, RuntimeUnixMicrosecondsV2,
    RuntimeWriterFenceGenerationV1,
};

const INGRESS_OPEN_ACKNOWLEDGEMENT_REQUEST_DOMAIN_V2: &[u8] =
    b"starring.runtime.ingress_open_acknowledgement_request.v2\0";
const MIN_INGRESS_OPEN_ACKNOWLEDGEMENT_LEASE_MILLISECONDS: u64 = 1_000;
const MAX_INGRESS_OPEN_ACKNOWLEDGEMENT_LEASE_MILLISECONDS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeIngressOpenAcknowledgementErrorV2 {
    #[error("runtime ingress acknowledgement lease has sub-millisecond precision")]
    LeaseSubMillisecond,
    #[error("runtime ingress acknowledgement lease is outside the supported range")]
    LeaseOutOfRange,
    #[error("runtime ingress acknowledgement integer exceeds the persistence range")]
    PersistenceIntegerOutOfRange,
    #[error("runtime ingress acknowledgement timestamp is not canonical")]
    TimestampNotCanonical,
    #[error("runtime ingress acknowledgement gateway was not explicitly resumed")]
    ExplicitResumeMissing,
    #[error("runtime ingress acknowledgement process identity does not match")]
    ProcessMismatch,
    #[error("runtime ingress acknowledgement owner lease is not current")]
    OwnerNotCurrent,
    #[error("runtime ingress acknowledgement revision is not the exact successor")]
    RevisionMismatch,
    #[error("runtime ingress acknowledgement interval is invalid")]
    IntervalInvalid,
    #[error("runtime ingress acknowledgement exceeds the owner lease")]
    OwnerLeaseExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeIngressOpenAcknowledgementLeaseDurationV2(u64);

impl RuntimeIngressOpenAcknowledgementLeaseDurationV2 {
    pub fn from_duration(
        value: Duration,
    ) -> Result<Self, RuntimeIngressOpenAcknowledgementErrorV2> {
        if !value.subsec_nanos().is_multiple_of(1_000_000) {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::LeaseSubMillisecond);
        }
        let milliseconds = value.as_millis();
        if milliseconds < u128::from(MIN_INGRESS_OPEN_ACKNOWLEDGEMENT_LEASE_MILLISECONDS)
            || milliseconds > u128::from(MAX_INGRESS_OPEN_ACKNOWLEDGEMENT_LEASE_MILLISECONDS)
        {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::LeaseOutOfRange);
        }
        Ok(Self(milliseconds as u64))
    }

    pub const fn from_milliseconds(
        value: u64,
    ) -> Result<Self, RuntimeIngressOpenAcknowledgementErrorV2> {
        if value < MIN_INGRESS_OPEN_ACKNOWLEDGEMENT_LEASE_MILLISECONDS
            || value > MAX_INGRESS_OPEN_ACKNOWLEDGEMENT_LEASE_MILLISECONDS
        {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::LeaseOutOfRange);
        }
        Ok(Self(value))
    }

    pub const fn milliseconds(self) -> u64 {
        self.0
    }

    pub const fn duration(self) -> Duration {
        Duration::from_millis(self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeIngressOpenAcknowledgementRequestDigestV2([u8; 32]);

impl RuntimeIngressOpenAcknowledgementRequestDigestV2 {
    pub const fn from_bytes(value: [u8; 32]) -> Self {
        Self(value)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for RuntimeIngressOpenAcknowledgementRequestDigestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressOpenAcknowledgementRequestDigestV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePublishIngressOpenAcknowledgementInputV2 {
    pub source_acknowledgement_revision: Option<NonZeroU64>,
    pub fence_generation: RuntimeWriterFenceGenerationV1,
    pub maintenance_gate_generation: NonZeroU64,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub gateway_ready: RuntimeGatewayReadyAttestationV2,
    pub lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePublishIngressOpenAcknowledgementV2 {
    input: RuntimePublishIngressOpenAcknowledgementInputV2,
    canonical_request_bytes: Vec<u8>,
    request_digest: RuntimeIngressOpenAcknowledgementRequestDigestV2,
}

impl RuntimePublishIngressOpenAcknowledgementV2 {
    pub fn new(
        input: RuntimePublishIngressOpenAcknowledgementInputV2,
    ) -> Result<Self, RuntimeIngressOpenAcknowledgementErrorV2> {
        validate_publish_input(&input)?;
        let canonical_request_bytes = canonical_publish_request_bytes(&input)?;
        let request_digest = RuntimeIngressOpenAcknowledgementRequestDigestV2::from_bytes(
            Sha256::digest(&canonical_request_bytes).into(),
        );
        Ok(Self {
            input,
            canonical_request_bytes,
            request_digest,
        })
    }

    pub fn source_acknowledgement_revision(&self) -> Option<NonZeroU64> {
        self.input.source_acknowledgement_revision
    }

    pub fn fence_generation(&self) -> RuntimeWriterFenceGenerationV1 {
        self.input.fence_generation
    }

    pub fn maintenance_gate_generation(&self) -> NonZeroU64 {
        self.input.maintenance_gate_generation
    }

    pub fn owner_receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.input.owner_receipt
    }

    pub fn gateway_ready(&self) -> &RuntimeGatewayReadyAttestationV2 {
        &self.input.gateway_ready
    }

    pub fn lease_for(&self) -> RuntimeIngressOpenAcknowledgementLeaseDurationV2 {
        self.input.lease_for
    }

    pub fn canonical_request_bytes(&self) -> &[u8] {
        &self.canonical_request_bytes
    }

    pub fn request_digest(&self) -> RuntimeIngressOpenAcknowledgementRequestDigestV2 {
        self.request_digest
    }

    pub fn observation_request(&self) -> RuntimeObserveIngressOpenAcknowledgementV2 {
        RuntimeObserveIngressOpenAcknowledgementV2 {
            gateway_shard_id: self.input.owner_receipt.lease_id.gateway_shard_id.clone(),
        }
    }
}

impl Debug for RuntimePublishIngressOpenAcknowledgementV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePublishIngressOpenAcknowledgementV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeIngressOpenAcknowledgementInputV2 {
    pub fence_generation: RuntimeWriterFenceGenerationV1,
    pub maintenance_gate_generation: NonZeroU64,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub resume_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub acknowledgement_revision: NonZeroU64,
    pub acknowledged_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeIngressOpenAcknowledgementV2 {
    input: RuntimeIngressOpenAcknowledgementInputV2,
}

impl RuntimeIngressOpenAcknowledgementV2 {
    pub fn new(
        input: RuntimeIngressOpenAcknowledgementInputV2,
    ) -> Result<Self, RuntimeIngressOpenAcknowledgementErrorV2> {
        validate_persistence_number(input.fence_generation.get())?;
        validate_persistence_number(input.maintenance_gate_generation.get())?;
        validate_persistence_number(input.gateway_owner_lease_id.lease_epoch.get())?;
        validate_persistence_number(input.observed_owner_revision.get())?;
        validate_persistence_number(input.connection_epoch.get())?;
        validate_persistence_number(input.admission_revision.get())?;
        validate_persistence_number(input.connected_event_sequence.get())?;
        validate_persistence_number(input.resume_sequence.get())?;
        validate_persistence_number(input.acknowledgement_revision.get())?;
        validate_timestamp(input.acknowledged_at)?;
        validate_timestamp(input.expires_at)?;
        if input.gateway_owner_lease_id.process_instance_id != input.process_instance_id {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::ProcessMismatch);
        }
        if input.resume_sequence <= input.connected_event_sequence {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::ExplicitResumeMissing);
        }
        if input.acknowledged_at >= input.expires_at {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::IntervalInvalid);
        }
        Ok(Self { input })
    }

    pub fn fence_generation(&self) -> RuntimeWriterFenceGenerationV1 {
        self.input.fence_generation
    }

    pub fn maintenance_gate_generation(&self) -> NonZeroU64 {
        self.input.maintenance_gate_generation
    }

    pub fn gateway_owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.input.gateway_owner_lease_id
    }

    pub fn observed_owner_revision(&self) -> NonZeroU64 {
        self.input.observed_owner_revision
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.input.process_instance_id
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.input.connection_epoch
    }

    pub fn admission_revision(&self) -> NonZeroU64 {
        self.input.admission_revision
    }

    pub fn connected_event_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.input.connected_event_sequence
    }

    pub fn resume_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.input.resume_sequence
    }

    pub fn acknowledgement_revision(&self) -> NonZeroU64 {
        self.input.acknowledgement_revision
    }

    pub fn acknowledged_at(&self) -> DateTime<Utc> {
        self.input.acknowledged_at
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.input.expires_at
    }
}

impl Debug for RuntimeIngressOpenAcknowledgementV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressOpenAcknowledgementV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeIngressOpenAcknowledgementReceiptInputV2 {
    pub source_acknowledgement_revision: Option<NonZeroU64>,
    pub request_digest: RuntimeIngressOpenAcknowledgementRequestDigestV2,
    pub acknowledgement: RuntimeIngressOpenAcknowledgementV2,
    pub observed_database_now: DateTime<Utc>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeIngressOpenAcknowledgementReceiptV2 {
    input: RuntimeIngressOpenAcknowledgementReceiptInputV2,
}

impl RuntimeIngressOpenAcknowledgementReceiptV2 {
    pub fn new(
        input: RuntimeIngressOpenAcknowledgementReceiptInputV2,
    ) -> Result<Self, RuntimeIngressOpenAcknowledgementErrorV2> {
        if let Some(revision) = input.source_acknowledgement_revision {
            validate_persistence_number(revision.get())?;
        }
        validate_timestamp(input.observed_database_now)?;
        let expected_revision = successor_revision(input.source_acknowledgement_revision)?;
        if input.acknowledgement.acknowledgement_revision() != expected_revision {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::RevisionMismatch);
        }
        if input.acknowledgement.acknowledged_at() > input.observed_database_now {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::IntervalInvalid);
        }
        Ok(Self { input })
    }

    pub fn source_acknowledgement_revision(&self) -> Option<NonZeroU64> {
        self.input.source_acknowledgement_revision
    }

    pub fn request_digest(&self) -> RuntimeIngressOpenAcknowledgementRequestDigestV2 {
        self.input.request_digest
    }

    pub fn acknowledgement(&self) -> &RuntimeIngressOpenAcknowledgementV2 {
        &self.input.acknowledgement
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.input.observed_database_now
    }

    pub fn into_acknowledgement(self) -> RuntimeIngressOpenAcknowledgementV2 {
        self.input.acknowledgement
    }
}

impl Debug for RuntimeIngressOpenAcknowledgementReceiptV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressOpenAcknowledgementReceiptV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObserveIngressOpenAcknowledgementV2 {
    pub gateway_shard_id: GatewayShardIdV1,
}

#[derive(Clone, PartialEq, Eq)]
pub enum RuntimeObservedIngressOpenAcknowledgementV2 {
    Missing {
        gateway_shard_id: GatewayShardIdV1,
        observed_database_now: DateTime<Utc>,
    },
    Present(Box<RuntimeIngressOpenAcknowledgementReceiptV2>),
}

impl RuntimeObservedIngressOpenAcknowledgementV2 {
    pub fn missing(
        gateway_shard_id: GatewayShardIdV1,
        observed_database_now: DateTime<Utc>,
    ) -> Result<Self, RuntimeIngressOpenAcknowledgementErrorV2> {
        validate_timestamp(observed_database_now)?;
        Ok(Self::Missing {
            gateway_shard_id,
            observed_database_now,
        })
    }

    pub fn present(receipt: RuntimeIngressOpenAcknowledgementReceiptV2) -> Self {
        Self::Present(Box::new(receipt))
    }

    pub fn gateway_shard_id(&self) -> &GatewayShardIdV1 {
        match self {
            Self::Missing {
                gateway_shard_id, ..
            } => gateway_shard_id,
            Self::Present(receipt) => {
                &receipt
                    .acknowledgement()
                    .gateway_owner_lease_id()
                    .gateway_shard_id
            }
        }
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        match self {
            Self::Missing {
                observed_database_now,
                ..
            } => *observed_database_now,
            Self::Present(receipt) => receipt.observed_database_now(),
        }
    }
}

impl Debug for RuntimeObservedIngressOpenAcknowledgementV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeObservedIngressOpenAcknowledgementV2(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum RuntimePublishIngressOpenAcknowledgementOutcomeV2 {
    Applied(RuntimeIngressOpenAcknowledgementReceiptV2),
    Replayed(RuntimeIngressOpenAcknowledgementReceiptV2),
    NotCurrent(RuntimeObservedIngressOpenAcknowledgementV2),
}

impl Debug for RuntimePublishIngressOpenAcknowledgementOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePublishIngressOpenAcknowledgementOutcomeV2(<redacted>)")
    }
}

fn validate_publish_input(
    input: &RuntimePublishIngressOpenAcknowledgementInputV2,
) -> Result<(), RuntimeIngressOpenAcknowledgementErrorV2> {
    if let Some(revision) = input.source_acknowledgement_revision {
        validate_persistence_number(revision.get())?;
        if revision.get() == i64::MAX as u64 {
            return Err(RuntimeIngressOpenAcknowledgementErrorV2::RevisionMismatch);
        }
    }
    validate_persistence_number(input.fence_generation.get())?;
    validate_persistence_number(input.maintenance_gate_generation.get())?;
    validate_persistence_number(input.owner_receipt.lease_id.lease_epoch.get())?;
    validate_persistence_number(input.owner_receipt.owner_revision.get())?;
    validate_persistence_number(input.gateway_ready.connection_epoch.get())?;
    validate_persistence_number(input.gateway_ready.admission_revision.get())?;
    validate_persistence_number(input.gateway_ready.connected_event_sequence.get())?;
    validate_persistence_number(input.gateway_ready.resume_sequence.get())?;
    validate_timestamp(input.owner_receipt.database_now)?;
    validate_timestamp(input.owner_receipt.expires_at)?;
    if input.owner_receipt.lease_id.process_instance_id != input.gateway_ready.process_instance_id {
        return Err(RuntimeIngressOpenAcknowledgementErrorV2::ProcessMismatch);
    }
    if !input.gateway_ready.was_explicitly_resumed() {
        return Err(RuntimeIngressOpenAcknowledgementErrorV2::ExplicitResumeMissing);
    }
    if input.owner_receipt.database_lease_duration().is_none() {
        return Err(RuntimeIngressOpenAcknowledgementErrorV2::OwnerNotCurrent);
    }
    Ok(())
}

fn canonical_publish_request_bytes(
    input: &RuntimePublishIngressOpenAcknowledgementInputV2,
) -> Result<Vec<u8>, RuntimeIngressOpenAcknowledgementErrorV2> {
    let mut bytes = Vec::with_capacity(512);
    push_frame(&mut bytes, INGRESS_OPEN_ACKNOWLEDGEMENT_REQUEST_DOMAIN_V2);
    match input.source_acknowledgement_revision {
        Some(revision) => {
            bytes.push(1);
            push_u64(&mut bytes, revision.get());
        }
        None => bytes.push(0),
    }
    push_u64(&mut bytes, input.fence_generation.get());
    push_u64(&mut bytes, input.maintenance_gate_generation.get());
    push_frame(
        &mut bytes,
        input
            .owner_receipt
            .lease_id
            .gateway_shard_id
            .as_str()
            .as_bytes(),
    );
    push_frame(
        &mut bytes,
        input
            .owner_receipt
            .lease_id
            .process_instance_id
            .as_str()
            .as_bytes(),
    );
    push_u64(&mut bytes, input.owner_receipt.lease_id.lease_epoch.get());
    push_frame(
        &mut bytes,
        input
            .owner_receipt
            .lease_id
            .expected_build_revision
            .as_str()
            .as_bytes(),
    );
    push_u64(&mut bytes, input.owner_receipt.owner_revision.get());
    push_i64(
        &mut bytes,
        RuntimeUnixMicrosecondsV2::from_datetime(input.owner_receipt.database_now)
            .map_err(|_| RuntimeIngressOpenAcknowledgementErrorV2::TimestampNotCanonical)?
            .get(),
    );
    push_i64(
        &mut bytes,
        RuntimeUnixMicrosecondsV2::from_datetime(input.owner_receipt.expires_at)
            .map_err(|_| RuntimeIngressOpenAcknowledgementErrorV2::TimestampNotCanonical)?
            .get(),
    );
    push_frame(
        &mut bytes,
        input.gateway_ready.process_instance_id.as_str().as_bytes(),
    );
    push_u64(&mut bytes, input.gateway_ready.connection_epoch.get());
    bytes.push(match input.gateway_ready.kind {
        crate::RuntimeGatewayReadyKindV2::Ready => 1,
        crate::RuntimeGatewayReadyKindV2::Resumed => 2,
    });
    push_u64(&mut bytes, input.gateway_ready.admission_revision.get());
    push_u64(
        &mut bytes,
        input.gateway_ready.connected_event_sequence.get(),
    );
    push_u64(&mut bytes, input.gateway_ready.resume_sequence.get());
    push_u64(&mut bytes, input.lease_for.milliseconds());
    Ok(bytes)
}

fn validate_persistence_number(value: u64) -> Result<(), RuntimeIngressOpenAcknowledgementErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(|_| ())
        .map_err(|_| RuntimeIngressOpenAcknowledgementErrorV2::PersistenceIntegerOutOfRange)
}

fn validate_timestamp(
    value: DateTime<Utc>,
) -> Result<(), RuntimeIngressOpenAcknowledgementErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(value)
        .map(|_| ())
        .map_err(|_| RuntimeIngressOpenAcknowledgementErrorV2::TimestampNotCanonical)
}

fn successor_revision(
    source: Option<NonZeroU64>,
) -> Result<NonZeroU64, RuntimeIngressOpenAcknowledgementErrorV2> {
    let value = match source {
        Some(revision) => revision
            .get()
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(RuntimeIngressOpenAcknowledgementErrorV2::RevisionMismatch)?,
        None => 1,
    };
    NonZeroU64::new(value).ok_or(RuntimeIngressOpenAcknowledgementErrorV2::RevisionMismatch)
}

fn push_frame(bytes: &mut Vec<u8>, value: &[u8]) {
    push_u64(
        bytes,
        u64::try_from(value.len()).expect("canonical ingress acknowledgement frame must fit u64"),
    );
    bytes.extend_from_slice(value);
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::time::Duration;

    use automation_runtime_convergence::ProcessInstanceId;
    use chrono::{DateTime, Utc};

    use super::{
        RuntimeIngressOpenAcknowledgementErrorV2, RuntimeIngressOpenAcknowledgementInputV2,
        RuntimeIngressOpenAcknowledgementLeaseDurationV2,
        RuntimeIngressOpenAcknowledgementReceiptInputV2,
        RuntimeIngressOpenAcknowledgementReceiptV2, RuntimeIngressOpenAcknowledgementV2,
        RuntimeObservedIngressOpenAcknowledgementV2,
        RuntimePublishIngressOpenAcknowledgementInputV2,
        RuntimePublishIngressOpenAcknowledgementV2,
    };
    use crate::{
        GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2,
        RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1,
        RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
        RuntimeWriterFenceGenerationV1,
    };

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn owner() -> RuntimeGatewayOwnerLeaseReceiptV1 {
        RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                lease_epoch: non_zero(3),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            owner_revision: non_zero(5),
            database_now: at(100),
            expires_at: at(130),
        }
    }

    fn ready() -> RuntimeGatewayReadyAttestationV2 {
        RuntimeGatewayReadyAttestationV2 {
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            connection_epoch: non_zero(7),
            kind: RuntimeGatewayReadyKindV2::Resumed,
            admission_revision: non_zero(11),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(17)),
        }
    }

    fn request(
        source_acknowledgement_revision: Option<NonZeroU64>,
    ) -> RuntimePublishIngressOpenAcknowledgementV2 {
        let mut input = publish_input();
        input.source_acknowledgement_revision = source_acknowledgement_revision;
        RuntimePublishIngressOpenAcknowledgementV2::new(input).unwrap()
    }

    fn publish_input() -> RuntimePublishIngressOpenAcknowledgementInputV2 {
        RuntimePublishIngressOpenAcknowledgementInputV2 {
            source_acknowledgement_revision: Some(non_zero(29)),
            fence_generation: RuntimeWriterFenceGenerationV1::new(non_zero(19)),
            maintenance_gate_generation: non_zero(23),
            owner_receipt: owner(),
            gateway_ready: ready(),
            lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(5_000)
                .unwrap(),
        }
    }

    fn acknowledgement(revision: u64) -> RuntimeIngressOpenAcknowledgementV2 {
        RuntimeIngressOpenAcknowledgementV2::new(RuntimeIngressOpenAcknowledgementInputV2 {
            fence_generation: RuntimeWriterFenceGenerationV1::new(non_zero(19)),
            maintenance_gate_generation: non_zero(23),
            gateway_owner_lease_id: owner().lease_id,
            observed_owner_revision: non_zero(5),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            connection_epoch: non_zero(7),
            admission_revision: non_zero(11),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(17)),
            acknowledgement_revision: non_zero(revision),
            acknowledged_at: at(101),
            expires_at: at(106),
        })
        .unwrap()
    }

    #[test]
    fn lease_accepts_only_exact_millisecond_values_between_one_and_ten_seconds() {
        for milliseconds in [1_000, 5_000, 10_000] {
            let lease =
                RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(milliseconds)
                    .unwrap();
            assert_eq!(lease.milliseconds(), milliseconds);
            assert_eq!(lease.duration(), Duration::from_millis(milliseconds));
        }
        for milliseconds in [0, 999, 10_001] {
            assert_eq!(
                RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(milliseconds),
                Err(RuntimeIngressOpenAcknowledgementErrorV2::LeaseOutOfRange)
            );
        }
        assert_eq!(
            RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(Duration::from_nanos(
                1_000_000_001
            )),
            Err(RuntimeIngressOpenAcknowledgementErrorV2::LeaseSubMillisecond)
        );
    }

    #[test]
    fn canonical_request_digest_is_stable_and_binds_every_replay_field() {
        let original = request(Some(non_zero(29)));
        let replay = request(Some(non_zero(29)));

        assert_eq!(original, replay);
        assert_eq!(
            original.request_digest().as_bytes(),
            &[
                209, 78, 79, 187, 167, 2, 182, 196, 159, 0, 138, 178, 236, 252, 30, 214, 81, 90,
                232, 108, 102, 81, 210, 181, 0, 175, 25, 161, 7, 84, 30, 222,
            ]
        );

        let mut mutations = Vec::new();

        let mut input = publish_input();
        input.source_acknowledgement_revision = Some(non_zero(30));
        mutations.push(("source revision", input));

        let mut input = publish_input();
        input.fence_generation = RuntimeWriterFenceGenerationV1::new(non_zero(20));
        mutations.push(("writer fence", input));

        let mut input = publish_input();
        input.maintenance_gate_generation = non_zero(24);
        mutations.push(("maintenance gate", input));

        let mut input = publish_input();
        input.owner_receipt.lease_id.gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
        mutations.push(("gateway shard", input));

        let mut input = publish_input();
        input.owner_receipt.lease_id.process_instance_id =
            ProcessInstanceId::parse("process:2").unwrap();
        input.gateway_ready.process_instance_id = ProcessInstanceId::parse("process:2").unwrap();
        mutations.push(("process", input));

        let mut input = publish_input();
        input.owner_receipt.lease_id.lease_epoch = non_zero(4);
        mutations.push(("owner lease epoch", input));

        let mut input = publish_input();
        input.owner_receipt.lease_id.expected_build_revision =
            RuntimeBuildRevisionV1::parse("build:2").unwrap();
        mutations.push(("build revision", input));

        let mut input = publish_input();
        input.owner_receipt.owner_revision = non_zero(6);
        mutations.push(("owner revision", input));

        let mut input = publish_input();
        input.owner_receipt.database_now = at(101);
        mutations.push(("owner database clock", input));

        let mut input = publish_input();
        input.owner_receipt.expires_at = at(131);
        mutations.push(("owner expiry", input));

        let mut input = publish_input();
        input.gateway_ready.connection_epoch = non_zero(8);
        mutations.push(("connection epoch", input));

        let mut input = publish_input();
        input.gateway_ready.kind = RuntimeGatewayReadyKindV2::Ready;
        mutations.push(("ready kind", input));

        let mut input = publish_input();
        input.gateway_ready.admission_revision = non_zero(12);
        mutations.push(("admission revision", input));

        let mut input = publish_input();
        input.gateway_ready.connected_event_sequence =
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(14));
        mutations.push(("connected sequence", input));

        let mut input = publish_input();
        input.gateway_ready.resume_sequence = RuntimeGatewayAdmissionSequenceV2::new(non_zero(18));
        mutations.push(("resume sequence", input));

        let mut input = publish_input();
        input.lease_for =
            RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(6_000).unwrap();
        mutations.push(("lease", input));

        for (field, input) in mutations {
            let changed = RuntimePublishIngressOpenAcknowledgementV2::new(input).unwrap();
            assert_ne!(
                changed.request_digest(),
                original.request_digest(),
                "{field}"
            );
            assert_ne!(
                changed.canonical_request_bytes(),
                original.canonical_request_bytes(),
                "{field}"
            );
        }
    }

    #[test]
    fn request_rejects_noncurrent_owner_unresumed_gateway_and_database_overflow() {
        let mut expired = owner();
        expired.expires_at = expired.database_now;
        let mut input = RuntimePublishIngressOpenAcknowledgementInputV2 {
            source_acknowledgement_revision: None,
            fence_generation: RuntimeWriterFenceGenerationV1::new(non_zero(19)),
            maintenance_gate_generation: non_zero(23),
            owner_receipt: expired,
            gateway_ready: ready(),
            lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_milliseconds(5_000)
                .unwrap(),
        };
        assert_eq!(
            RuntimePublishIngressOpenAcknowledgementV2::new(input.clone()),
            Err(RuntimeIngressOpenAcknowledgementErrorV2::OwnerNotCurrent)
        );
        input.owner_receipt = owner();
        input.gateway_ready.resume_sequence = input.gateway_ready.connected_event_sequence;
        assert_eq!(
            RuntimePublishIngressOpenAcknowledgementV2::new(input.clone()),
            Err(RuntimeIngressOpenAcknowledgementErrorV2::ExplicitResumeMissing)
        );
        input.gateway_ready = ready();
        input.maintenance_gate_generation = non_zero(i64::MAX as u64 + 1);
        assert_eq!(
            RuntimePublishIngressOpenAcknowledgementV2::new(input),
            Err(RuntimeIngressOpenAcknowledgementErrorV2::PersistenceIntegerOutOfRange)
        );
    }

    #[test]
    fn durable_acknowledgement_and_receipt_require_the_exact_successor() {
        let request = request(Some(non_zero(29)));
        let acknowledgement = acknowledgement(30);
        let receipt = RuntimeIngressOpenAcknowledgementReceiptV2::new(
            RuntimeIngressOpenAcknowledgementReceiptInputV2 {
                source_acknowledgement_revision: Some(non_zero(29)),
                request_digest: request.request_digest(),
                acknowledgement: acknowledgement.clone(),
                observed_database_now: at(102),
            },
        )
        .unwrap();

        assert_eq!(receipt.acknowledgement(), &acknowledgement);
        assert_eq!(
            receipt.source_acknowledgement_revision(),
            Some(non_zero(29))
        );
        assert_eq!(receipt.request_digest(), request.request_digest());
        assert_eq!(
            RuntimeIngressOpenAcknowledgementReceiptV2::new(
                RuntimeIngressOpenAcknowledgementReceiptInputV2 {
                    source_acknowledgement_revision: Some(non_zero(28)),
                    request_digest: request.request_digest(),
                    acknowledgement: acknowledgement.clone(),
                    observed_database_now: at(102),
                }
            ),
            Err(RuntimeIngressOpenAcknowledgementErrorV2::RevisionMismatch)
        );
    }

    #[test]
    fn expired_durable_observation_remains_decodable_for_worker_classification() {
        let request = request(Some(non_zero(29)));
        let expired_observation = RuntimeIngressOpenAcknowledgementReceiptV2::new(
            RuntimeIngressOpenAcknowledgementReceiptInputV2 {
                source_acknowledgement_revision: Some(non_zero(29)),
                request_digest: request.request_digest(),
                acknowledgement: acknowledgement(30),
                observed_database_now: at(106),
            },
        )
        .unwrap();
        assert_eq!(expired_observation.observed_database_now(), at(106));
        assert_eq!(expired_observation.acknowledgement().expires_at(), at(106));
        let observed = RuntimeObservedIngressOpenAcknowledgementV2::present(expired_observation);
        assert_eq!(observed.observed_database_now(), at(106));
    }

    #[test]
    fn missing_observation_is_scoped_and_uses_a_canonical_database_clock() {
        let shard = GatewayShardIdV1::parse("shard:0").unwrap();
        let observation =
            RuntimeObservedIngressOpenAcknowledgementV2::missing(shard.clone(), at(100)).unwrap();

        assert_eq!(observation.gateway_shard_id(), &shard);
        assert_eq!(observation.observed_database_now(), at(100));
        assert_eq!(
            format!("{observation:?}"),
            "RuntimeObservedIngressOpenAcknowledgementV2(<redacted>)"
        );
    }
}
