use std::time::Duration;

use crate::error::validate_millisecond_duration;
use crate::{RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1};

pub const MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT: Duration = Duration::from_millis(100);
pub const DEFAULT_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT: Duration = Duration::from_millis(400);
pub const MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionRouteTimeoutV1(Duration);

impl RuntimeInteractionRouteTimeoutV1 {
    pub fn new(timeout: Duration) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_millisecond_duration(timeout, MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT)?;
        if timeout < MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self(timeout))
    }

    pub fn duration(self) -> Duration {
        self.0
    }

    pub(crate) fn database_timeouts(
        self,
        configured: RuntimeInteractionDatabaseTimeoutsV1,
    ) -> Result<RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1> {
        let statement_millis = configured
            .statement_timeout()
            .as_millis()
            .min(self.0.as_millis().saturating_mul(3) / 4);
        let lock_millis = configured
            .lock_timeout()
            .as_millis()
            .min(statement_millis.saturating_mul(2) / 3);
        let statement_millis = u64::try_from(statement_millis)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let lock_millis = u64::try_from(lock_millis)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        RuntimeInteractionDatabaseTimeoutsV1::new(
            Duration::from_millis(statement_millis),
            Duration::from_millis(lock_millis),
        )
    }
}

impl Default for RuntimeInteractionRouteTimeoutV1 {
    fn default() -> Self {
        Self(DEFAULT_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_read_timeout_is_bounded_to_whole_milliseconds() {
        assert_eq!(
            RuntimeInteractionRouteTimeoutV1::default().duration(),
            Duration::from_millis(400)
        );
        assert_eq!(
            RuntimeInteractionRouteTimeoutV1::new(Duration::from_millis(250))
                .unwrap()
                .duration(),
            Duration::from_millis(250)
        );
        for invalid in [
            Duration::ZERO,
            Duration::from_nanos(1),
            Duration::from_millis(99),
            Duration::from_millis(2_001),
        ] {
            assert_eq!(
                RuntimeInteractionRouteTimeoutV1::new(invalid),
                Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
            );
        }
    }

    #[test]
    fn database_timeouts_leave_server_and_cleanup_headroom() {
        let route = RuntimeInteractionRouteTimeoutV1::default();
        let configured = RuntimeInteractionDatabaseTimeoutsV1::new(
            Duration::from_secs(2),
            Duration::from_secs(1),
        )
        .unwrap();
        let bounded = route.database_timeouts(configured).unwrap();
        assert_eq!(bounded.statement_timeout(), Duration::from_millis(300));
        assert_eq!(bounded.lock_timeout(), Duration::from_millis(200));
        assert!(bounded.lock_timeout() < bounded.statement_timeout());
        assert!(bounded.statement_timeout() < route.duration());

        let already_stricter = RuntimeInteractionDatabaseTimeoutsV1::new(
            Duration::from_millis(20),
            Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(
            route.database_timeouts(already_stricter).unwrap(),
            already_stricter
        );
    }
}
