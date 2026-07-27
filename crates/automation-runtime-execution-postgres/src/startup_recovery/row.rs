use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeStartupRecoveryObservationReceiptV2,
    RuntimeStartupRecoveryObservationRequestV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use crate::gateway_owner::MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION;
use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeStartupRecoveryObservationRowV2 {
    outcome_name: String,
    observed_gateway_shard_id: String,
    observed_process_instance_id: Option<String>,
    observed_lease_epoch: Option<i64>,
    observed_runtime_build_revision: Option<String>,
    observed_owner_revision: Option<i64>,
    database_now: DateTime<Utc>,
    observed_owner_expires_at: Option<DateTime<Utc>>,
    serving_state_name: Option<String>,
    serving_count: Option<i64>,
    serving_earliest_expiry: Option<DateTime<Utc>>,
    serving_retry_after_milliseconds: Option<i64>,
    recoverable_awaiting_certification_count: Option<i64>,
    suspended_local_effect_count: Option<i64>,
    pending_runtime_drain_intent_count: Option<i64>,
    acknowledged_product_handoff_count: Option<i64>,
}

pub(super) enum RuntimeStartupRecoveryObservationDecodeOutcomeV2 {
    Observed(Box<RuntimeStartupRecoveryObservationReceiptV2>),
    NotCurrent,
    Ambiguous,
}

impl RuntimeStartupRecoveryObservationRowV2 {
    pub(super) fn decode(
        self,
        request: &RuntimeStartupRecoveryObservationRequestV2,
    ) -> Result<RuntimeStartupRecoveryObservationDecodeOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        if self.observed_gateway_shard_id
            != request.gateway_owner_lease_id.gateway_shard_id.as_str()
        {
            return Err(invalid());
        }
        match self.outcome_name.as_str() {
            "observed" => self.decode_observed(request),
            "not_current" => self.decode_not_current(request),
            "ambiguous" => self.decode_ambiguous(request),
            _ => Err(invalid()),
        }
    }

    fn decode_observed(
        &self,
        request: &RuntimeStartupRecoveryObservationRequestV2,
    ) -> Result<RuntimeStartupRecoveryObservationDecodeOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        let owner_receipt = self.require_exact_fresh_owner(request)?;
        let state = RuntimeStartupRecoveryStateV2 {
            serving: self.decode_serving_state()?,
            recoverable_awaiting_certification_count: persisted_u32(
                self.recoverable_awaiting_certification_count,
            )?,
            suspended_local_effect_count: persisted_u32(self.suspended_local_effect_count)?,
            pending_runtime_drain_intent_count: persisted_u32(
                self.pending_runtime_drain_intent_count,
            )?,
            acknowledged_product_handoff_count: persisted_u32(
                self.acknowledged_product_handoff_count,
            )?,
        };
        Ok(RuntimeStartupRecoveryObservationDecodeOutcomeV2::Observed(
            Box::new(RuntimeStartupRecoveryObservationReceiptV2 {
                correlation: request.correlation.clone(),
                owner_receipt,
                state,
            }),
        ))
    }

    fn decode_not_current(
        &self,
        request: &RuntimeStartupRecoveryObservationRequestV2,
    ) -> Result<RuntimeStartupRecoveryObservationDecodeOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.require_no_state_payload()?;
        match self.owner_payload_shape() {
            RuntimeStartupRecoveryOwnerPayloadShapeV2::Empty => {}
            RuntimeStartupRecoveryOwnerPayloadShapeV2::Complete => {
                let owner = self.decode_owner()?;
                if owner
                    .database_lease_duration()
                    .is_some_and(|duration| duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION)
                {
                    return Err(invalid());
                }
                if owner_matches_request(&owner, request)
                    && owner.database_lease_duration().is_some()
                {
                    return Err(invalid());
                }
            }
            RuntimeStartupRecoveryOwnerPayloadShapeV2::Partial => return Err(invalid()),
        }
        Ok(RuntimeStartupRecoveryObservationDecodeOutcomeV2::NotCurrent)
    }

    fn decode_ambiguous(
        &self,
        request: &RuntimeStartupRecoveryObservationRequestV2,
    ) -> Result<RuntimeStartupRecoveryObservationDecodeOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        self.require_exact_fresh_owner(request)?;
        if self.serving_state_name.as_deref() != Some("ambiguous")
            || self.serving_count.is_some()
            || self.serving_earliest_expiry.is_some()
            || self.serving_retry_after_milliseconds.is_some()
            || self.recoverable_awaiting_certification_count.is_some()
            || self.suspended_local_effect_count.is_some()
            || self.pending_runtime_drain_intent_count.is_some()
            || self.acknowledged_product_handoff_count.is_some()
        {
            return Err(invalid());
        }
        Ok(RuntimeStartupRecoveryObservationDecodeOutcomeV2::Ambiguous)
    }

    fn require_exact_fresh_owner(
        &self,
        request: &RuntimeStartupRecoveryObservationRequestV2,
    ) -> Result<RuntimeGatewayOwnerLeaseReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        if self.owner_payload_shape() != RuntimeStartupRecoveryOwnerPayloadShapeV2::Complete {
            return Err(invalid());
        }
        let owner = self.decode_owner()?;
        if !owner_matches_request(&owner, request)
            || owner
                .database_lease_duration()
                .is_none_or(|duration| duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION)
        {
            return Err(invalid());
        }
        Ok(owner)
    }

    fn decode_owner(
        &self,
    ) -> Result<RuntimeGatewayOwnerLeaseReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        Ok(RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse(&self.observed_gateway_shard_id)
                    .map_err(|_| invalid())?,
                process_instance_id: ProcessInstanceId::parse(
                    self.observed_process_instance_id
                        .as_deref()
                        .ok_or_else(invalid)?,
                )
                .map_err(|_| invalid())?,
                lease_epoch: persisted_positive_u64(self.observed_lease_epoch)?,
                expected_build_revision: RuntimeBuildRevisionV1::parse(
                    self.observed_runtime_build_revision
                        .as_deref()
                        .ok_or_else(invalid)?,
                )
                .map_err(|_| invalid())?,
            },
            owner_revision: persisted_positive_u64(self.observed_owner_revision)?,
            database_now: self.database_now,
            expires_at: self.observed_owner_expires_at.ok_or_else(invalid)?,
        })
    }

    fn decode_serving_state(
        &self,
    ) -> Result<RuntimeStartupServingStateV2, RuntimeExecutionPersistenceErrorV1> {
        match self.serving_state_name.as_deref() {
            Some("empty") => {
                if self.serving_count != Some(0)
                    || self.serving_earliest_expiry.is_some()
                    || self.serving_retry_after_milliseconds.is_some()
                {
                    return Err(invalid());
                }
                Ok(RuntimeStartupServingStateV2::Empty)
            }
            Some("recoverable_stale") => {
                let count = persisted_positive_u32(self.serving_count)?;
                if self.serving_earliest_expiry.is_some()
                    || self.serving_retry_after_milliseconds.is_some()
                {
                    return Err(invalid());
                }
                Ok(RuntimeStartupServingStateV2::RecoverableStale { count })
            }
            Some("foreign_fresh") => {
                let count = persisted_positive_u32(self.serving_count)?;
                let earliest_expiry = self.serving_earliest_expiry.ok_or_else(invalid)?;
                let delta_milliseconds = earliest_expiry
                    .signed_duration_since(self.database_now)
                    .num_milliseconds();
                let expected_retry_milliseconds = delta_milliseconds.min(1_000);
                let retry_milliseconds =
                    self.serving_retry_after_milliseconds.ok_or_else(invalid)?;
                if expected_retry_milliseconds < 1
                    || retry_milliseconds != expected_retry_milliseconds
                {
                    return Err(invalid());
                }
                let retry_after = Duration::from_millis(
                    u64::try_from(retry_milliseconds).map_err(|_| invalid())?,
                );
                Ok(RuntimeStartupServingStateV2::ForeignFresh {
                    count,
                    database_now: self.database_now,
                    earliest_expiry,
                    retry_after,
                })
            }
            _ => Err(invalid()),
        }
    }

    fn require_no_state_payload(&self) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        if self.serving_state_name.is_none()
            && self.serving_count.is_none()
            && self.serving_earliest_expiry.is_none()
            && self.serving_retry_after_milliseconds.is_none()
            && self.recoverable_awaiting_certification_count.is_none()
            && self.suspended_local_effect_count.is_none()
            && self.pending_runtime_drain_intent_count.is_none()
            && self.acknowledged_product_handoff_count.is_none()
        {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    fn owner_payload_shape(&self) -> RuntimeStartupRecoveryOwnerPayloadShapeV2 {
        let fields = [
            self.observed_process_instance_id.is_some(),
            self.observed_lease_epoch.is_some(),
            self.observed_runtime_build_revision.is_some(),
            self.observed_owner_revision.is_some(),
            self.observed_owner_expires_at.is_some(),
        ];
        if fields.iter().all(|present| *present) {
            RuntimeStartupRecoveryOwnerPayloadShapeV2::Complete
        } else if fields.iter().all(|present| !*present) {
            RuntimeStartupRecoveryOwnerPayloadShapeV2::Empty
        } else {
            RuntimeStartupRecoveryOwnerPayloadShapeV2::Partial
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeStartupRecoveryOwnerPayloadShapeV2 {
    Empty,
    Complete,
    Partial,
}

fn owner_matches_request(
    owner: &RuntimeGatewayOwnerLeaseReceiptV1,
    request: &RuntimeStartupRecoveryObservationRequestV2,
) -> bool {
    owner.lease_id == request.gateway_owner_lease_id
        && owner.owner_revision == request.expected_owner_revision
        && owner.expires_at == request.expected_owner_expires_at
}

fn persisted_u32(value: Option<i64>) -> Result<u32, RuntimeExecutionPersistenceErrorV1> {
    u32::try_from(value.ok_or_else(invalid)?).map_err(|_| invalid())
}

fn persisted_positive_u32(value: Option<i64>) -> Result<u32, RuntimeExecutionPersistenceErrorV1> {
    let value = persisted_u32(value)?;
    if value == 0 {
        Err(invalid())
    } else {
        Ok(value)
    }
}

fn persisted_positive_u64(
    value: Option<i64>,
) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    let value = u64::try_from(value.ok_or_else(invalid)?).map_err(|_| invalid())?;
    NonZeroU64::new(value).ok_or_else(invalid)
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_runtime_controller::{
        RuntimeRecoveryIdV2, RuntimeStartupRecoveryObservationCorrelationV2,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn request() -> RuntimeStartupRecoveryObservationRequestV2 {
        RuntimeStartupRecoveryObservationRequestV2 {
            correlation: RuntimeStartupRecoveryObservationCorrelationV2 {
                recovery_id: RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef")
                    .unwrap(),
                originating_emergency_generation: non_zero(2),
                coordinator_generation: non_zero(3),
                authority_revision: non_zero(4),
            },
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                lease_epoch: non_zero(5),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            expected_owner_revision: non_zero(6),
            expected_owner_expires_at: at(200),
        }
    }

    fn observed_row(serving_state_name: &str) -> RuntimeStartupRecoveryObservationRowV2 {
        RuntimeStartupRecoveryObservationRowV2 {
            outcome_name: "observed".to_owned(),
            observed_gateway_shard_id: "shard:0".to_owned(),
            observed_process_instance_id: Some("process:1".to_owned()),
            observed_lease_epoch: Some(5),
            observed_runtime_build_revision: Some("build:1".to_owned()),
            observed_owner_revision: Some(6),
            database_now: at(100),
            observed_owner_expires_at: Some(at(200)),
            serving_state_name: Some(serving_state_name.to_owned()),
            serving_count: Some(0),
            serving_earliest_expiry: None,
            serving_retry_after_milliseconds: None,
            recoverable_awaiting_certification_count: Some(1),
            suspended_local_effect_count: Some(2),
            pending_runtime_drain_intent_count: Some(3),
            acknowledged_product_handoff_count: Some(4),
        }
    }

    fn assert_corrupt(
        result: Result<
            RuntimeStartupRecoveryObservationDecodeOutcomeV2,
            RuntimeExecutionPersistenceErrorV1,
        >,
    ) {
        assert!(matches!(
            result,
            Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn observed_empty_decodes_exact_owner_correlation_and_counts() {
        let request = request();
        let decoded = observed_row("empty").decode(&request).unwrap();
        let RuntimeStartupRecoveryObservationDecodeOutcomeV2::Observed(receipt) = decoded else {
            panic!("unexpected outcome");
        };
        assert_eq!(receipt.correlation, request.correlation);
        assert_eq!(
            receipt.owner_receipt.lease_id,
            request.gateway_owner_lease_id
        );
        assert_eq!(receipt.owner_receipt.owner_revision, non_zero(6));
        assert_eq!(receipt.owner_receipt.database_now, at(100));
        assert!(matches!(
            receipt.state.serving,
            RuntimeStartupServingStateV2::Empty
        ));
        assert_eq!(receipt.state.recoverable_awaiting_certification_count, 1);
        assert_eq!(receipt.state.suspended_local_effect_count, 2);
        assert_eq!(receipt.state.pending_runtime_drain_intent_count, 3);
        assert_eq!(receipt.state.acknowledged_product_handoff_count, 4);
    }

    #[test]
    fn observed_recoverable_stale_requires_a_positive_bounded_count() {
        let request = request();
        let mut row = observed_row("recoverable_stale");
        row.serving_count = Some(7);
        let decoded = row.decode(&request).unwrap();
        let RuntimeStartupRecoveryObservationDecodeOutcomeV2::Observed(receipt) = decoded else {
            panic!("unexpected outcome");
        };
        assert!(matches!(
            receipt.state.serving,
            RuntimeStartupServingStateV2::RecoverableStale { count: 7 }
        ));

        for count in [Some(0), Some(-1), Some(i64::from(u32::MAX) + 1), None] {
            let mut row = observed_row("recoverable_stale");
            row.serving_count = count;
            assert_corrupt(row.decode(&request));
        }
    }

    #[test]
    fn observed_foreign_fresh_requires_the_exact_retry_shape() {
        let request = request();
        let mut row = observed_row("foreign_fresh");
        row.serving_count = Some(2);
        row.serving_earliest_expiry = Some(at(102));
        row.serving_retry_after_milliseconds = Some(1_000);
        let decoded = row.decode(&request).unwrap();
        let RuntimeStartupRecoveryObservationDecodeOutcomeV2::Observed(receipt) = decoded else {
            panic!("unexpected outcome");
        };
        assert!(matches!(
            receipt.state.serving,
            RuntimeStartupServingStateV2::ForeignFresh {
                count: 2,
                database_now,
                earliest_expiry,
                retry_after,
            } if database_now == at(100)
                && earliest_expiry == at(102)
                && retry_after == Duration::from_secs(1)
        ));

        for retry in [Some(999), Some(1_001), Some(0), None] {
            let mut row = observed_row("foreign_fresh");
            row.serving_count = Some(2);
            row.serving_earliest_expiry = Some(at(102));
            row.serving_retry_after_milliseconds = retry;
            assert_corrupt(row.decode(&request));
        }
    }

    #[test]
    fn observed_rejects_owner_counter_and_serving_shape_drift() {
        let request = request();
        let mut wrong_owner = observed_row("empty");
        wrong_owner.observed_owner_revision = Some(7);
        assert_corrupt(wrong_owner.decode(&request));

        let mut expired = observed_row("empty");
        expired.observed_owner_expires_at = Some(at(100));
        assert_corrupt(expired.decode(&request));

        let mut oversized = observed_row("empty");
        oversized.observed_owner_expires_at = Some(at(401));
        assert_corrupt(oversized.decode(&request));

        let mut missing_count = observed_row("empty");
        missing_count.suspended_local_effect_count = None;
        assert_corrupt(missing_count.decode(&request));

        let mut oversized_count = observed_row("empty");
        oversized_count.pending_runtime_drain_intent_count = Some(i64::from(u32::MAX) + 1);
        assert_corrupt(oversized_count.decode(&request));

        let mut stale_expiry = observed_row("recoverable_stale");
        stale_expiry.serving_count = Some(1);
        stale_expiry.serving_earliest_expiry = Some(at(101));
        assert_corrupt(stale_expiry.decode(&request));
    }

    #[test]
    fn not_current_accepts_only_empty_or_complete_noncurrent_owner_shapes() {
        let request = request();
        let mut empty = observed_row("empty");
        empty.outcome_name = "not_current".to_owned();
        empty.observed_process_instance_id = None;
        empty.observed_lease_epoch = None;
        empty.observed_runtime_build_revision = None;
        empty.observed_owner_revision = None;
        empty.observed_owner_expires_at = None;
        empty.serving_state_name = None;
        empty.serving_count = None;
        empty.recoverable_awaiting_certification_count = None;
        empty.suspended_local_effect_count = None;
        empty.pending_runtime_drain_intent_count = None;
        empty.acknowledged_product_handoff_count = None;
        assert!(matches!(
            empty.clone().decode(&request).unwrap(),
            RuntimeStartupRecoveryObservationDecodeOutcomeV2::NotCurrent
        ));

        let mut foreign = observed_row("empty");
        foreign.outcome_name = "not_current".to_owned();
        foreign.observed_process_instance_id = Some("process:2".to_owned());
        foreign.serving_state_name = None;
        foreign.serving_count = None;
        foreign.recoverable_awaiting_certification_count = None;
        foreign.suspended_local_effect_count = None;
        foreign.pending_runtime_drain_intent_count = None;
        foreign.acknowledged_product_handoff_count = None;
        assert!(matches!(
            foreign.clone().decode(&request).unwrap(),
            RuntimeStartupRecoveryObservationDecodeOutcomeV2::NotCurrent
        ));

        let mut exact_fresh = foreign.clone();
        exact_fresh.observed_process_instance_id = Some("process:1".to_owned());
        assert_corrupt(exact_fresh.decode(&request));

        let mut partial = empty;
        partial.observed_owner_revision = Some(6);
        assert_corrupt(partial.decode(&request));
    }

    #[test]
    fn ambiguous_never_synthesizes_state_or_accepts_payload() {
        let request = request();
        let mut ambiguous = observed_row("ambiguous");
        ambiguous.outcome_name = "ambiguous".to_owned();
        ambiguous.serving_count = None;
        ambiguous.recoverable_awaiting_certification_count = None;
        ambiguous.suspended_local_effect_count = None;
        ambiguous.pending_runtime_drain_intent_count = None;
        ambiguous.acknowledged_product_handoff_count = None;
        assert!(matches!(
            ambiguous.clone().decode(&request).unwrap(),
            RuntimeStartupRecoveryObservationDecodeOutcomeV2::Ambiguous
        ));

        ambiguous.suspended_local_effect_count = Some(0);
        assert_corrupt(ambiguous.decode(&request));
    }

    #[test]
    fn unknown_outcome_and_wrong_shard_fail_closed() {
        let request = request();
        let mut unknown = observed_row("empty");
        unknown.outcome_name = "future".to_owned();
        assert_corrupt(unknown.decode(&request));

        let mut wrong_shard = observed_row("empty");
        wrong_shard.observed_gateway_shard_id = "shard:1".to_owned();
        assert_corrupt(wrong_shard.decode(&request));
    }
}
