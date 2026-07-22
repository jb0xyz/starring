use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeBuildRevisionV1,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseObservationV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeObservedGatewayOwnerLeaseV1,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseOutcomeV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use super::MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION;
use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeGatewayOwnerOperationRowV1 {
    outcome_name: String,
    gateway_shard_id: String,
    process_instance_id: Option<String>,
    lease_epoch: Option<i64>,
    expected_build_revision: Option<String>,
    owner_revision: Option<i64>,
    database_now: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}

impl RuntimeGatewayOwnerOperationRowV1 {
    pub(crate) fn decode_observation(
        self,
    ) -> Result<RuntimeGatewayOwnerLeaseObservationV1, RuntimeExecutionPersistenceErrorV1> {
        if self.outcome_name == "unowned" {
            self.require_unowned()
        } else if self.outcome_name == "owned" {
            self.require_owned_observation()
        } else {
            Err(invalid())
        }
    }

    pub(crate) fn decode_acquire(
        self,
    ) -> Result<RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeExecutionPersistenceErrorV1> {
        match self.outcome_name.as_str() {
            "acquired" => Ok(RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(
                self.require_mutation_receipt()?,
            )),
            "contended" => Ok(RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(
                self.require_mutation_receipt()?,
            )),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn decode_renew(
        self,
    ) -> Result<RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeExecutionPersistenceErrorV1> {
        match self.outcome_name.as_str() {
            "renewed" => Ok(RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(
                self.require_mutation_receipt()?,
            )),
            "not_current" => Ok(RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(
                self.require_optional_observation()?,
            )),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn decode_release(
        self,
    ) -> Result<RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeExecutionPersistenceErrorV1> {
        match self.outcome_name.as_str() {
            "released" => {
                let database_now = self.database_now;
                let lease_id = self.require_released_identity()?;
                Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                    lease_id,
                    database_now,
                })
            }
            "not_held" => Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(
                self.require_optional_observation()?,
            )),
            _ => Err(invalid()),
        }
    }

    fn require_optional_observation(
        self,
    ) -> Result<RuntimeGatewayOwnerLeaseObservationV1, RuntimeExecutionPersistenceErrorV1> {
        if self.all_owner_fields_absent() {
            self.require_unowned()
        } else {
            self.require_owned_observation()
        }
    }

    fn require_unowned(
        self,
    ) -> Result<RuntimeGatewayOwnerLeaseObservationV1, RuntimeExecutionPersistenceErrorV1> {
        if !self.all_owner_fields_absent() {
            return Err(invalid());
        }
        Ok(RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id: parse_shard(&self.gateway_shard_id)?,
            database_now: self.database_now,
        })
    }

    fn require_owned_observation(
        self,
    ) -> Result<RuntimeGatewayOwnerLeaseObservationV1, RuntimeExecutionPersistenceErrorV1> {
        let expires_at = self.expires_at.ok_or_else(invalid)?;
        let observed = RuntimeObservedGatewayOwnerLeaseV1 {
            lease_id: self.parse_lease_id()?,
            owner_revision: positive(self.owner_revision)?,
            observed_database_now: self.database_now,
            expires_at,
        };
        if observed
            .current_receipt()
            .and_then(|receipt| receipt.database_lease_duration())
            .is_none_or(|duration| duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION)
        {
            return Err(invalid());
        }
        Ok(RuntimeGatewayOwnerLeaseObservationV1::Owned(observed))
    }

    fn require_mutation_receipt(
        self,
    ) -> Result<RuntimeGatewayOwnerLeaseReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let expires_at = self.expires_at.ok_or_else(invalid)?;
        let receipt = RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: self.parse_lease_id()?,
            owner_revision: positive(self.owner_revision)?,
            database_now: self.database_now,
            expires_at,
        };
        if receipt
            .database_lease_duration()
            .is_none_or(|duration| duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION)
        {
            return Err(invalid());
        }
        Ok(receipt)
    }

    fn require_released_identity(
        self,
    ) -> Result<RuntimeGatewayOwnerLeaseIdV1, RuntimeExecutionPersistenceErrorV1> {
        if self.owner_revision.is_some() || self.expires_at.is_some() {
            return Err(invalid());
        }
        self.parse_lease_id()
    }

    fn parse_lease_id(
        &self,
    ) -> Result<RuntimeGatewayOwnerLeaseIdV1, RuntimeExecutionPersistenceErrorV1> {
        Ok(RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: parse_shard(&self.gateway_shard_id)?,
            process_instance_id: ProcessInstanceId::parse(
                self.process_instance_id.as_deref().ok_or_else(invalid)?,
            )
            .map_err(|_| invalid())?,
            lease_epoch: positive(self.lease_epoch)?,
            expected_build_revision: RuntimeBuildRevisionV1::parse(
                self.expected_build_revision
                    .as_deref()
                    .ok_or_else(invalid)?,
            )
            .map_err(|_| invalid())?,
        })
    }

    fn all_owner_fields_absent(&self) -> bool {
        self.process_instance_id.is_none()
            && self.lease_epoch.is_none()
            && self.expected_build_revision.is_none()
            && self.owner_revision.is_none()
            && self.expires_at.is_none()
    }
}

fn parse_shard(value: &str) -> Result<GatewayShardIdV1, RuntimeExecutionPersistenceErrorV1> {
    GatewayShardIdV1::parse(value).map_err(|_| invalid())
}

fn positive(value: Option<i64>) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    let value = value.ok_or_else(invalid)?;
    let value = u64::try_from(value).map_err(|_| invalid())?;
    NonZeroU64::new(value).ok_or_else(invalid)
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

    fn row(outcome_name: &str) -> RuntimeGatewayOwnerOperationRowV1 {
        RuntimeGatewayOwnerOperationRowV1 {
            outcome_name: outcome_name.to_owned(),
            gateway_shard_id: "shard:0".to_owned(),
            process_instance_id: Some("process:1".to_owned()),
            lease_epoch: Some(7),
            expected_build_revision: Some("build:1".to_owned()),
            owner_revision: Some(3),
            database_now: at(100),
            expires_at: Some(at(130)),
        }
    }

    fn unowned(outcome_name: &str) -> RuntimeGatewayOwnerOperationRowV1 {
        RuntimeGatewayOwnerOperationRowV1 {
            outcome_name: outcome_name.to_owned(),
            gateway_shard_id: "shard:0".to_owned(),
            process_instance_id: None,
            lease_epoch: None,
            expected_build_revision: None,
            owner_revision: None,
            database_now: at(100),
            expires_at: None,
        }
    }

    #[test]
    fn observation_rows_require_exact_owned_or_unowned_shapes() {
        assert!(matches!(
            row("owned").decode_observation().unwrap(),
            RuntimeGatewayOwnerLeaseObservationV1::Owned(_)
        ));
        assert!(matches!(
            unowned("unowned").decode_observation().unwrap(),
            RuntimeGatewayOwnerLeaseObservationV1::Unowned { .. }
        ));
        let mut mixed = unowned("unowned");
        mixed.lease_epoch = Some(7);
        assert_eq!(mixed.decode_observation(), Err(invalid()));
        assert_eq!(row("unknown").decode_observation(), Err(invalid()));
    }

    #[test]
    fn mutation_rows_reject_nonfresh_or_nonpositive_receipts() {
        let mut expired = row("acquired");
        expired.expires_at = Some(expired.database_now);
        assert_eq!(expired.decode_acquire(), Err(invalid()));
        let mut zero_revision = row("renewed");
        zero_revision.owner_revision = Some(0);
        assert_eq!(zero_revision.decode_renew(), Err(invalid()));
        let mut zero_epoch = row("contended");
        zero_epoch.lease_epoch = Some(0);
        assert_eq!(zero_epoch.decode_acquire(), Err(invalid()));
        let mut unbounded = row("acquired");
        unbounded.expires_at = Some(at(401));
        assert_eq!(unbounded.decode_acquire(), Err(invalid()));
    }

    #[test]
    fn release_rows_echo_only_stable_identity_or_current_observation() {
        let mut released = row("released");
        released.owner_revision = None;
        released.expires_at = None;
        let outcome = released.decode_release().unwrap();
        assert!(matches!(
            outcome,
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { .. }
        ));
        assert!(matches!(
            row("not_held").decode_release().unwrap(),
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(
                RuntimeGatewayOwnerLeaseObservationV1::Owned(_)
            )
        ));
        assert!(matches!(
            unowned("not_held").decode_release().unwrap(),
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(
                RuntimeGatewayOwnerLeaseObservationV1::Unowned { .. }
            )
        ));
    }

    #[test]
    fn persisted_identity_text_is_revalidated() {
        for mutate in [
            |row: &mut RuntimeGatewayOwnerOperationRowV1| row.gateway_shard_id = "".to_owned(),
            |row: &mut RuntimeGatewayOwnerOperationRowV1| {
                row.process_instance_id = Some("".to_owned())
            },
            |row: &mut RuntimeGatewayOwnerOperationRowV1| {
                row.expected_build_revision = Some("".to_owned())
            },
        ] {
            let mut forged = row("acquired");
            mutate(&mut forged);
            assert_eq!(forged.decode_acquire(), Err(invalid()));
        }
    }
}
