use std::time::Duration;

const DEFAULT_STATEMENT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDeploymentStatusConfigError {
    #[error("product deployment-status statement timeout is invalid")]
    InvalidStatementTimeout,
    #[error("product deployment-status lock timeout is invalid")]
    InvalidLockTimeout,
    #[error("product deployment-status idle transaction timeout is invalid")]
    InvalidIdleTransactionTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostgresProductDeploymentStatusesConfig {
    statement_timeout: Duration,
    lock_timeout: Duration,
    idle_transaction_timeout: Duration,
}

impl PostgresProductDeploymentStatusesConfig {
    pub fn new(
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, ProductDeploymentStatusConfigError> {
        if !bounded_millisecond_duration(statement_timeout, MAX_STATEMENT_TIMEOUT) {
            return Err(ProductDeploymentStatusConfigError::InvalidStatementTimeout);
        }
        if !bounded_millisecond_duration(lock_timeout, MAX_LOCK_TIMEOUT)
            || lock_timeout >= statement_timeout
        {
            return Err(ProductDeploymentStatusConfigError::InvalidLockTimeout);
        }
        let idle_transaction_timeout = statement_timeout
            .checked_mul(2)
            .filter(|value| *value <= MAX_STATEMENT_TIMEOUT.checked_mul(2).unwrap())
            .ok_or(ProductDeploymentStatusConfigError::InvalidIdleTransactionTimeout)?;
        Ok(Self {
            statement_timeout,
            lock_timeout,
            idle_transaction_timeout,
        })
    }

    pub(crate) fn statement_timeout(self) -> String {
        duration_setting(self.statement_timeout)
    }

    pub(crate) fn lock_timeout(self) -> String {
        duration_setting(self.lock_timeout)
    }

    pub(crate) fn idle_transaction_timeout(self) -> String {
        duration_setting(self.idle_transaction_timeout)
    }
}

impl Default for PostgresProductDeploymentStatusesConfig {
    fn default() -> Self {
        Self::new(DEFAULT_STATEMENT_TIMEOUT, DEFAULT_LOCK_TIMEOUT)
            .expect("default product deployment-status configuration is valid")
    }
}

fn bounded_millisecond_duration(value: Duration, maximum: Duration) -> bool {
    !value.is_zero()
        && value <= maximum
        && value.subsec_nanos().is_multiple_of(1_000_000)
        && value.as_millis() <= i64::MAX as u128
}

fn duration_setting(value: Duration) -> String {
    format!("{}ms", value.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_is_bounded_and_millisecond_exact() {
        let config = PostgresProductDeploymentStatusesConfig::new(
            Duration::from_millis(800),
            Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(config.statement_timeout(), "800ms");
        assert_eq!(config.lock_timeout(), "200ms");
        assert_eq!(config.idle_transaction_timeout(), "1600ms");
        assert!(PostgresProductDeploymentStatusesConfig::new(
            Duration::ZERO,
            Duration::from_millis(1)
        )
        .is_err());
        assert!(PostgresProductDeploymentStatusesConfig::new(
            Duration::from_millis(100),
            Duration::from_millis(100)
        )
        .is_err());
        assert!(PostgresProductDeploymentStatusesConfig::new(
            Duration::from_nanos(1_000_001),
            Duration::from_millis(1)
        )
        .is_err());
    }
}
