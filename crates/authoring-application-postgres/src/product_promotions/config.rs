use std::fmt::{Debug, Formatter};
use std::time::Duration;

use crate::product_action_digest::ProductActionDigestKeyringV1;

const MAX_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TRANSACTION_RETRIES: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductPromotionConfigError {
    #[error("product promotion statement timeout is invalid")]
    InvalidStatementTimeout,
    #[error("product promotion lock timeout is invalid")]
    InvalidLockTimeout,
    #[error("product promotion transaction retry limit is invalid")]
    InvalidTransactionRetryLimit,
}

#[derive(Clone)]
pub struct PostgresProductPromotionsConfig {
    #[allow(dead_code)]
    keyring: ProductActionDigestKeyringV1,
    #[allow(dead_code)]
    statement_timeout: Duration,
    #[allow(dead_code)]
    lock_timeout: Duration,
    #[allow(dead_code)]
    transaction_retry_limit: u8,
}

impl PostgresProductPromotionsConfig {
    pub fn new(
        keyring: ProductActionDigestKeyringV1,
        statement_timeout: Duration,
        lock_timeout: Duration,
        transaction_retry_limit: u8,
    ) -> Result<Self, ProductPromotionConfigError> {
        if !bounded_millisecond_duration(statement_timeout, MAX_STATEMENT_TIMEOUT) {
            return Err(ProductPromotionConfigError::InvalidStatementTimeout);
        }
        if !bounded_millisecond_duration(lock_timeout, MAX_LOCK_TIMEOUT)
            || lock_timeout >= statement_timeout
        {
            return Err(ProductPromotionConfigError::InvalidLockTimeout);
        }
        if transaction_retry_limit > MAX_TRANSACTION_RETRIES {
            return Err(ProductPromotionConfigError::InvalidTransactionRetryLimit);
        }
        Ok(Self {
            keyring,
            statement_timeout,
            lock_timeout,
            transaction_retry_limit,
        })
    }

    pub fn production(
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductPromotionConfigError> {
        Self::new(
            keyring,
            Duration::from_secs(2),
            Duration::from_millis(500),
            2,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn keyring(&self) -> &ProductActionDigestKeyringV1 {
        &self.keyring
    }

    #[allow(dead_code)]
    pub(crate) fn statement_timeout(&self) -> String {
        millisecond_setting(self.statement_timeout)
    }

    #[allow(dead_code)]
    pub(crate) fn lock_timeout(&self) -> String {
        millisecond_setting(self.lock_timeout)
    }

    #[allow(dead_code)]
    pub(crate) fn idle_transaction_timeout(&self) -> String {
        millisecond_setting(self.statement_timeout)
    }

    #[allow(dead_code)]
    pub(crate) fn transaction_retry_limit(&self) -> u8 {
        self.transaction_retry_limit
    }
}

impl Debug for PostgresProductPromotionsConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PostgresProductPromotionsConfig(<redacted>)")
    }
}

fn bounded_millisecond_duration(value: Duration, maximum: Duration) -> bool {
    !value.is_zero()
        && value <= maximum
        && value.as_millis() <= i64::MAX as u128
        && value.subsec_nanos().is_multiple_of(1_000_000)
}

#[allow(dead_code)]
fn millisecond_setting(value: Duration) -> String {
    format!("{}ms", value.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_action_digest::ProductActionDigestKeyV1;

    fn keyring() -> ProductActionDigestKeyringV1 {
        let key = ProductActionDigestKeyV1::from_bytes(
            "active-v1",
            std::array::from_fn(|index| index as u8),
        )
        .unwrap();
        ProductActionDigestKeyringV1::new(key, []).unwrap()
    }

    #[test]
    fn production_configuration_is_bounded_and_redacted() {
        let config = PostgresProductPromotionsConfig::production(keyring()).unwrap();
        assert_eq!(config.statement_timeout(), "2000ms");
        assert_eq!(config.lock_timeout(), "500ms");
        assert_eq!(config.idle_transaction_timeout(), "2000ms");
        assert_eq!(config.transaction_retry_limit(), 2);
        assert_eq!(
            format!("{config:?}"),
            "PostgresProductPromotionsConfig(<redacted>)"
        );
    }

    #[test]
    fn configuration_rejects_unbounded_or_ambiguous_values() {
        assert_eq!(
            PostgresProductPromotionsConfig::new(
                keyring(),
                Duration::from_secs(31),
                Duration::from_millis(1),
                0,
            )
            .unwrap_err(),
            ProductPromotionConfigError::InvalidStatementTimeout
        );
        assert_eq!(
            PostgresProductPromotionsConfig::new(
                keyring(),
                Duration::from_secs(1),
                Duration::from_secs(1),
                0,
            )
            .unwrap_err(),
            ProductPromotionConfigError::InvalidLockTimeout
        );
        assert_eq!(
            PostgresProductPromotionsConfig::new(
                keyring(),
                Duration::from_secs(1),
                Duration::from_millis(1),
                4,
            )
            .unwrap_err(),
            ProductPromotionConfigError::InvalidTransactionRetryLimit
        );
        assert_eq!(
            PostgresProductPromotionsConfig::new(
                keyring(),
                Duration::from_nanos(1),
                Duration::from_nanos(1),
                0,
            )
            .unwrap_err(),
            ProductPromotionConfigError::InvalidStatementTimeout
        );
    }
}
