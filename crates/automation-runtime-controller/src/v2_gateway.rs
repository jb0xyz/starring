use std::num::NonZeroU64;
use std::time::Duration;

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

impl RuntimeGatewayOwnerLeaseReceiptV1 {
    pub fn database_lease_duration(&self) -> Option<Duration> {
        let duration = self
            .expires_at
            .signed_duration_since(self.database_now)
            .to_std()
            .ok()?;
        if duration.is_zero() {
            None
        } else {
            Some(duration)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeGatewayOwnerLeaseDurationV1(Duration);

impl RuntimeGatewayOwnerLeaseDurationV1 {
    pub fn new(value: Duration) -> Option<Self> {
        if value.is_zero() {
            None
        } else {
            Some(Self(value))
        }
    }

    pub fn get(self) -> Duration {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeAcquireGatewayOwnerLeaseV1 {
    pub gateway_shard_id: GatewayShardIdV1,
    pub process_instance_id: ProcessInstanceId,
    pub expected_build_revision: RuntimeBuildRevisionV1,
    pub lease_for: RuntimeGatewayOwnerLeaseDurationV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRenewGatewayOwnerLeaseV1 {
    pub lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub expected_owner_revision: NonZeroU64,
    pub lease_for: RuntimeGatewayOwnerLeaseDurationV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeReleaseGatewayOwnerLeaseV1 {
    pub lease_id: RuntimeGatewayOwnerLeaseIdV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObserveGatewayOwnerLeaseV1 {
    pub gateway_shard_id: GatewayShardIdV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObservedGatewayOwnerLeaseV1 {
    pub lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: NonZeroU64,
    pub observed_database_now: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl RuntimeObservedGatewayOwnerLeaseV1 {
    pub fn current_receipt(&self) -> Option<RuntimeGatewayOwnerLeaseReceiptV1> {
        let receipt = RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: self.lease_id.clone(),
            owner_revision: self.owner_revision,
            database_now: self.observed_database_now,
            expires_at: self.expires_at,
        };
        receipt.database_lease_duration().map(|_| receipt)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerLeaseObservationV1 {
    Unowned {
        gateway_shard_id: GatewayShardIdV1,
        database_now: DateTime<Utc>,
    },
    Owned(RuntimeObservedGatewayOwnerLeaseV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeAcquireGatewayOwnerLeaseOutcomeV1 {
    Acquired(RuntimeGatewayOwnerLeaseReceiptV1),
    Contended(RuntimeGatewayOwnerLeaseReceiptV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeRenewGatewayOwnerLeaseOutcomeV1 {
    Renewed(RuntimeGatewayOwnerLeaseReceiptV1),
    NotCurrent(RuntimeGatewayOwnerLeaseObservationV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeReleaseGatewayOwnerLeaseOutcomeV1 {
    Released {
        lease_id: RuntimeGatewayOwnerLeaseIdV1,
        database_now: DateTime<Utc>,
    },
    NotHeld(RuntimeGatewayOwnerLeaseObservationV1),
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
        self.resume_sequence.get() > self.connected_event_sequence.get()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_convergence::ProcessInstanceId;
    use chrono::{DateTime, Utc};

    use super::{
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
        RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
        RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
        RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
        RuntimeObserveGatewayOwnerLeaseV1, RuntimeObservedGatewayOwnerLeaseV1,
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
        RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
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

    fn observed(
        owner_revision: u64,
        observed_database_now: i64,
        expires_at: i64,
    ) -> RuntimeObservedGatewayOwnerLeaseV1 {
        RuntimeObservedGatewayOwnerLeaseV1 {
            lease_id: lease_id(),
            owner_revision: non_zero(owner_revision),
            observed_database_now: at(observed_database_now),
            expires_at: at(expires_at),
        }
    }

    fn lease_for(seconds: u64) -> RuntimeGatewayOwnerLeaseDurationV1 {
        RuntimeGatewayOwnerLeaseDurationV1::new(std::time::Duration::from_secs(seconds)).unwrap()
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
    fn owner_lease_duration_uses_only_the_database_interval() {
        assert_eq!(
            receipt(3, 100, 120).database_lease_duration(),
            Some(std::time::Duration::from_secs(20))
        );
        assert_eq!(receipt(3, 100, 100).database_lease_duration(), None);
        assert_eq!(receipt(3, 101, 100).database_lease_duration(), None);
        assert_eq!(
            observed(3, 100, 120).current_receipt(),
            Some(receipt(3, 100, 120))
        );
        assert_eq!(observed(3, 120, 120).current_receipt(), None);
    }

    #[test]
    fn owner_lease_duration_rejects_zero_at_construction() {
        assert_eq!(
            RuntimeGatewayOwnerLeaseDurationV1::new(std::time::Duration::ZERO),
            None
        );
        assert_eq!(lease_for(30).get(), std::time::Duration::from_secs(30));
    }

    #[test]
    fn owner_persistence_requests_bind_the_exact_authority_fields() {
        let acquire = RuntimeAcquireGatewayOwnerLeaseV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            lease_for: lease_for(30),
        };
        let renew = RuntimeRenewGatewayOwnerLeaseV1 {
            lease_id: lease_id(),
            expected_owner_revision: non_zero(3),
            lease_for: lease_for(30),
        };
        let release = RuntimeReleaseGatewayOwnerLeaseV1 {
            lease_id: lease_id(),
        };
        let observe = RuntimeObserveGatewayOwnerLeaseV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        };

        assert_eq!(acquire.process_instance_id.as_str(), "process:1");
        assert_eq!(renew.expected_owner_revision, non_zero(3));
        assert_eq!(release.lease_id, lease_id());
        assert_eq!(observe.gateway_shard_id.as_str(), "shard:0");
    }

    #[test]
    fn owner_mutation_outcomes_are_closed_and_preserve_observation() {
        let receipt = receipt(3, 100, 120);
        let unowned = RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            database_now: at(100),
        };
        let acquire = RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(receipt.clone());
        let renew = RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(unowned.clone());
        let release = RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(unowned.clone());

        assert_eq!(
            acquire,
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(receipt)
        );
        assert_eq!(
            renew,
            RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(unowned.clone())
        );
        assert_eq!(
            release,
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(unowned)
        );
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

        let ready_after_explicit_resume = RuntimeGatewayReadyAttestationV2 {
            kind: RuntimeGatewayReadyKindV2::Ready,
            ..explicit.clone()
        };
        assert!(ready_after_explicit_resume.was_explicitly_resumed());

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
