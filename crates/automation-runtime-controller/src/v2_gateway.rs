use std::num::NonZeroU64;

use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use crate::{GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeGatewayOwnerLeaseIdV1 {
    pub gateway_shard_id: GatewayShardIdV1,
    pub process_instance_id: ProcessInstanceId,
    pub lease_epoch: NonZeroU64,
    pub expected_build_revision: RuntimeBuildRevisionV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerLeaseReceiptV1 {
    pub lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: NonZeroU64,
    pub database_now: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeGatewayReadyKindV2 {
    Ready,
    Resumed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayReadyAttestationV2 {
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub kind: RuntimeGatewayReadyKindV2,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub resume_sequence: RuntimeGatewayAdmissionSequenceV2,
}

impl RuntimeGatewayReadyAttestationV2 {
    pub fn was_explicitly_resumed(&self) -> bool {
        self.kind == RuntimeGatewayReadyKindV2::Resumed
            && self.resume_sequence.get() > self.connected_event_sequence.get()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_convergence::ProcessInstanceId;
    use chrono::{DateTime, Utc};

    use super::{
        RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1,
        RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
    };
    use crate::{GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2};

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn lease_id() -> RuntimeGatewayOwnerLeaseIdV1 {
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            lease_epoch: non_zero(7),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        }
    }

    fn receipt(
        owner_revision: u64,
        database_now: i64,
        expires_at: i64,
    ) -> RuntimeGatewayOwnerLeaseReceiptV1 {
        RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: lease_id(),
            owner_revision: non_zero(owner_revision),
            database_now: at(database_now),
            expires_at: at(expires_at),
        }
    }

    fn ready() -> RuntimeGatewayReadyAttestationV2 {
        RuntimeGatewayReadyAttestationV2 {
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            connection_epoch: non_zero(11),
            kind: RuntimeGatewayReadyKindV2::Resumed,
            admission_revision: non_zero(13),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(17)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(19)),
        }
    }

    #[test]
    fn owner_lease_identity_is_the_exact_stable_tuple() {
        let expected = lease_id();

        assert_eq!(expected, expected.clone());

        let mut shard = expected.clone();
        shard.gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
        assert_ne!(expected, shard);

        let mut process = expected.clone();
        process.process_instance_id = ProcessInstanceId::parse("process:2").unwrap();
        assert_ne!(expected, process);

        let mut epoch = expected.clone();
        epoch.lease_epoch = non_zero(8);
        assert_ne!(expected, epoch);

        let mut build = expected.clone();
        build.expected_build_revision = RuntimeBuildRevisionV1::parse("build:2").unwrap();
        assert_ne!(expected, build);
    }

    #[test]
    fn owner_lease_identity_remains_stable_across_newer_renewal_receipts() {
        let initial = receipt(3, 100, 120);
        let renewed = receipt(7, 110, 140);

        assert_eq!(initial.lease_id, renewed.lease_id);
        assert!(renewed.owner_revision > initial.owner_revision);
        assert_ne!(initial.database_now, renewed.database_now);
        assert_ne!(initial.expires_at, renewed.expires_at);
    }

    #[test]
    fn ready_kind_is_a_closed_two_variant_contract() {
        fn name(kind: RuntimeGatewayReadyKindV2) -> &'static str {
            match kind {
                RuntimeGatewayReadyKindV2::Ready => "ready",
                RuntimeGatewayReadyKindV2::Resumed => "resumed",
            }
        }

        assert_eq!(name(RuntimeGatewayReadyKindV2::Ready), "ready");
        assert_eq!(name(RuntimeGatewayReadyKindV2::Resumed), "resumed");
    }

    #[test]
    fn ready_evidence_requires_strict_resume_order_to_qualify() {
        let explicit = ready();
        assert!(explicit.was_explicitly_resumed());

        let ready_without_resume_kind = RuntimeGatewayReadyAttestationV2 {
            kind: RuntimeGatewayReadyKindV2::Ready,
            ..explicit.clone()
        };
        assert!(!ready_without_resume_kind.was_explicitly_resumed());

        let legacy_equal = RuntimeGatewayReadyAttestationV2 {
            resume_sequence: explicit.connected_event_sequence,
            ..explicit.clone()
        };
        assert!(!legacy_equal.was_explicitly_resumed());

        let reverse = RuntimeGatewayReadyAttestationV2 {
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(20)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(19)),
            ..explicit
        };
        assert!(!reverse.was_explicitly_resumed());
    }

    #[test]
    fn ready_evidence_identity_changes_with_every_field() {
        let expected = ready();

        let mut process = expected.clone();
        process.process_instance_id = ProcessInstanceId::parse("process:2").unwrap();
        assert_ne!(expected, process);

        let mut epoch = expected.clone();
        epoch.connection_epoch = non_zero(12);
        assert_ne!(expected, epoch);

        let mut kind = expected.clone();
        kind.kind = RuntimeGatewayReadyKindV2::Ready;
        assert_ne!(expected, kind);

        let mut revision = expected.clone();
        revision.admission_revision = non_zero(14);
        assert_ne!(expected, revision);

        let mut connected = expected.clone();
        connected.connected_event_sequence = RuntimeGatewayAdmissionSequenceV2::new(non_zero(18));
        assert_ne!(expected, connected);

        let mut resumed = expected.clone();
        resumed.resume_sequence = RuntimeGatewayAdmissionSequenceV2::new(non_zero(20));
        assert_ne!(expected, resumed);
    }
}
