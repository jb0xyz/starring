use std::num::NonZeroU64;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::{RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeRecoveryIdV2};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryObservationCorrelationV2 {
    pub recovery_id: RuntimeRecoveryIdV2,
    pub originating_emergency_generation: NonZeroU64,
    pub coordinator_generation: NonZeroU64,
    pub authority_revision: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryObservationRequestV2 {
    pub correlation: RuntimeStartupRecoveryObservationCorrelationV2,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub expected_owner_revision: NonZeroU64,
    pub expected_owner_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryObservationReceiptV2 {
    pub correlation: RuntimeStartupRecoveryObservationCorrelationV2,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub state: RuntimeStartupRecoveryStateV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStartupServingStateV2 {
    Empty,
    RecoverableStale {
        count: u32,
    },
    ForeignFresh {
        count: u32,
        database_now: DateTime<Utc>,
        earliest_expiry: DateTime<Utc>,
        retry_after: Duration,
    },
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStartupRecoveryStateV2 {
    pub serving: RuntimeStartupServingStateV2,
    pub recoverable_awaiting_certification_count: u32,
    pub suspended_local_effect_count: u32,
    pub pending_runtime_drain_intent_count: u32,
    pub acknowledged_product_handoff_count: u32,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::time::Duration;

    use automation_runtime_convergence::ProcessInstanceId;
    use chrono::{DateTime, Utc};

    use crate::{
        GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseIdV1,
        RuntimeGatewayOwnerLeaseReceiptV1, RuntimeRecoveryIdV2,
    };

    use super::{
        RuntimeStartupRecoveryObservationCorrelationV2, RuntimeStartupRecoveryObservationReceiptV2,
        RuntimeStartupRecoveryObservationRequestV2, RuntimeStartupRecoveryStateV2,
        RuntimeStartupServingStateV2,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn correlation() -> RuntimeStartupRecoveryObservationCorrelationV2 {
        RuntimeStartupRecoveryObservationCorrelationV2 {
            recovery_id: RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
            originating_emergency_generation: non_zero(2),
            coordinator_generation: non_zero(3),
            authority_revision: non_zero(4),
        }
    }

    fn owner_receipt(database_now: DateTime<Utc>) -> RuntimeGatewayOwnerLeaseReceiptV1 {
        RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                lease_epoch: non_zero(5),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            owner_revision: non_zero(6),
            database_now,
            expires_at: at(200),
        }
    }

    #[test]
    fn startup_recovery_state_preserves_every_closed_observation_class() {
        let state = RuntimeStartupRecoveryStateV2 {
            serving: RuntimeStartupServingStateV2::ForeignFresh {
                count: 2,
                database_now: at(100),
                earliest_expiry: at(110),
                retry_after: Duration::from_secs(5),
            },
            recoverable_awaiting_certification_count: 3,
            suspended_local_effect_count: 4,
            pending_runtime_drain_intent_count: 5,
            acknowledged_product_handoff_count: 6,
        };

        assert_eq!(state.recoverable_awaiting_certification_count, 3);
        assert_eq!(state.suspended_local_effect_count, 4);
        assert_eq!(state.pending_runtime_drain_intent_count, 5);
        assert_eq!(state.acknowledged_product_handoff_count, 6);
        assert!(matches!(
            state.serving,
            RuntimeStartupServingStateV2::ForeignFresh { count: 2, .. }
        ));
    }

    #[test]
    fn startup_observation_contract_preserves_exact_owner_and_call_correlation() {
        let owner = owner_receipt(at(100));
        let request = RuntimeStartupRecoveryObservationRequestV2 {
            correlation: correlation(),
            gateway_owner_lease_id: owner.lease_id.clone(),
            expected_owner_revision: owner.owner_revision,
            expected_owner_expires_at: owner.expires_at,
        };
        let receipt = RuntimeStartupRecoveryObservationReceiptV2 {
            correlation: request.correlation.clone(),
            owner_receipt: owner_receipt(at(101)),
            state: RuntimeStartupRecoveryStateV2 {
                serving: RuntimeStartupServingStateV2::Empty,
                recoverable_awaiting_certification_count: 0,
                suspended_local_effect_count: 0,
                pending_runtime_drain_intent_count: 0,
                acknowledged_product_handoff_count: 7,
            },
        };

        assert_eq!(
            request.correlation.recovery_id.as_str(),
            "0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            request.correlation.originating_emergency_generation,
            non_zero(2)
        );
        assert_eq!(request.correlation.coordinator_generation, non_zero(3));
        assert_eq!(request.correlation.authority_revision, non_zero(4));
        assert_eq!(request.gateway_owner_lease_id, owner.lease_id);
        assert_eq!(request.expected_owner_revision, owner.owner_revision);
        assert_eq!(request.expected_owner_expires_at, at(200));
        assert_eq!(receipt.correlation, request.correlation);
        assert_eq!(receipt.owner_receipt.database_now, at(101));
        assert_eq!(receipt.state.acknowledged_product_handoff_count, 7);
    }
}
