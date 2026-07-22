use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeCutoverCoordinatorIdV1, RuntimeObservedWriterFenceClosedV1,
    RuntimeWriterFenceClosedLeaseIdV1, RuntimeWriterFenceGenerationV1,
    RuntimeWriterFenceObservationV1,
};
use chrono::{DateTime, Utc};

use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct RuntimeWriterFenceObservationRowV1 {
    fence_state: String,
    fence_generation: i64,
    cutover_coordinator_id: Option<String>,
    cutover_lease_epoch: Option<i64>,
    database_now: DateTime<Utc>,
    cutover_expires_at: Option<DateTime<Utc>>,
}

impl RuntimeWriterFenceObservationRowV1 {
    pub(crate) fn decode(
        self,
    ) -> Result<RuntimeWriterFenceObservationV1, RuntimeExecutionPersistenceErrorV1> {
        let generation = RuntimeWriterFenceGenerationV1::new(positive(self.fence_generation)?);
        match self.fence_state.as_str() {
            "open"
                if self.cutover_coordinator_id.is_none()
                    && self.cutover_lease_epoch.is_none()
                    && self.cutover_expires_at.is_none() =>
            {
                Ok(RuntimeWriterFenceObservationV1::Open {
                    generation,
                    observed_database_now: self.database_now,
                })
            }
            "closed" => Ok(RuntimeWriterFenceObservationV1::Closed(
                RuntimeObservedWriterFenceClosedV1 {
                    lease_id: RuntimeWriterFenceClosedLeaseIdV1 {
                        generation,
                        coordinator_id: RuntimeCutoverCoordinatorIdV1::parse(
                            self.cutover_coordinator_id.ok_or_else(invalid)?,
                        )
                        .map_err(|_| invalid())?,
                        lease_epoch: positive(self.cutover_lease_epoch.ok_or_else(invalid)?)?,
                    },
                    observed_database_now: self.database_now,
                    expires_at: self.cutover_expires_at.ok_or_else(invalid)?,
                },
            )),
            _ => Err(invalid()),
        }
    }
}

fn positive(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
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

    fn open() -> RuntimeWriterFenceObservationRowV1 {
        RuntimeWriterFenceObservationRowV1 {
            fence_state: "open".to_owned(),
            fence_generation: 1,
            cutover_coordinator_id: None,
            cutover_lease_epoch: None,
            database_now: at(100),
            cutover_expires_at: None,
        }
    }

    fn closed() -> RuntimeWriterFenceObservationRowV1 {
        RuntimeWriterFenceObservationRowV1 {
            fence_state: "closed".to_owned(),
            fence_generation: 2,
            cutover_coordinator_id: Some("00112233445566778899aabbccddeeff".to_owned()),
            cutover_lease_epoch: Some(1),
            database_now: at(100),
            cutover_expires_at: Some(at(130)),
        }
    }

    #[test]
    fn open_row_requires_the_exact_null_shape() {
        assert!(matches!(
            open().decode().unwrap(),
            RuntimeWriterFenceObservationV1::Open { .. }
        ));
        let mut coordinator = open();
        coordinator.cutover_coordinator_id = Some("00112233445566778899aabbccddeeff".to_owned());
        assert_eq!(coordinator.decode(), Err(invalid()));
        let mut epoch = open();
        epoch.cutover_lease_epoch = Some(1);
        assert_eq!(epoch.decode(), Err(invalid()));
        let mut expiry = open();
        expiry.cutover_expires_at = Some(at(130));
        assert_eq!(expiry.decode(), Err(invalid()));
    }

    #[test]
    fn closed_row_requires_exact_canonical_identity_and_positive_counters() {
        let decoded = closed().decode().unwrap();
        let RuntimeWriterFenceObservationV1::Closed(observed) = decoded else {
            panic!("closed row decoded as open")
        };
        assert_eq!(observed.lease_id.generation.get(), 2);
        assert_eq!(observed.lease_id.lease_epoch.get(), 1);
        assert!(observed.current_lease().is_some());

        let mut uppercase = closed();
        uppercase.cutover_coordinator_id = Some("00112233445566778899AABBCCDDEEFF".to_owned());
        assert_eq!(uppercase.decode(), Err(invalid()));
        let mut zero_generation = closed();
        zero_generation.fence_generation = 0;
        assert_eq!(zero_generation.decode(), Err(invalid()));
        let mut zero_epoch = closed();
        zero_epoch.cutover_lease_epoch = Some(0);
        assert_eq!(zero_epoch.decode(), Err(invalid()));
    }

    #[test]
    fn expired_closed_row_remains_a_closed_observation() {
        let mut expired = closed();
        expired.cutover_expires_at = Some(expired.database_now);
        let RuntimeWriterFenceObservationV1::Closed(observed) = expired.decode().unwrap() else {
            panic!("expired closed row decoded as open")
        };
        assert!(observed.current_lease().is_none());
    }

    #[test]
    fn unknown_or_mixed_rows_are_rejected() {
        let mut unknown = open();
        unknown.fence_state = "unknown".to_owned();
        assert_eq!(unknown.decode(), Err(invalid()));
        let mut missing = closed();
        missing.cutover_expires_at = None;
        assert_eq!(missing.decode(), Err(invalid()));
    }
}
