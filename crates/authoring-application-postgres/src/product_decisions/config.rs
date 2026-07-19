use std::fmt::{Debug, Formatter};
use std::time::Duration;

use crate::product_action_digest::ProductActionDigestKeyringV1;

const MAX_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDecisionConfigError {
    #[error("product decision statement timeout is invalid")]
    InvalidStatementTimeout,
    #[error("product decision lock timeout is invalid")]
    InvalidLockTimeout,
}

#[derive(Clone)]
pub struct PostgresProductDecisionsConfig {
    keyring: ProductActionDigestKeyringV1,
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl PostgresProductDecisionsConfig {
    pub fn new(
        keyring: ProductActionDigestKeyringV1,
        statement_timeout: Duration,
        lock_timeout: Duration,
    ) -> Result<Self, ProductDecisionConfigError> {
        if statement_timeout.is_zero()
            || statement_timeout > MAX_STATEMENT_TIMEOUT
            || statement_timeout.as_millis() > i64::MAX as u128
        {
            return Err(ProductDecisionConfigError::InvalidStatementTimeout);
        }
        if lock_timeout.is_zero()
            || lock_timeout > MAX_LOCK_TIMEOUT
            || lock_timeout >= statement_timeout
            || lock_timeout.as_millis() > i64::MAX as u128
        {
            return Err(ProductDecisionConfigError::InvalidLockTimeout);
        }
        Ok(Self {
            keyring,
            statement_timeout,
            lock_timeout,
        })
    }

    pub fn production(
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Self::new(keyring, Duration::from_secs(2), Duration::from_millis(500))
    }

    pub(crate) fn keyring(&self) -> &ProductActionDigestKeyringV1 {
        &self.keyring
    }

    pub(crate) fn statement_timeout(&self) -> String {
        format!("{}ms", self.statement_timeout.as_millis())
    }

    pub(crate) fn lock_timeout(&self) -> String {
        format!("{}ms", self.lock_timeout.as_millis())
    }
}

impl Debug for PostgresProductDecisionsConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PostgresProductDecisionsConfig(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_action_digest::{ProductActionDigestKeyV1, ProductActionDigestKeyringError};

    fn key(id: &str, byte: u8) -> ProductActionDigestKeyV1 {
        ProductActionDigestKeyV1::from_bytes(
            id,
            std::array::from_fn(|index| byte.wrapping_add(index as u8)),
        )
        .unwrap()
    }

    #[test]
    fn timeouts_reject_ambiguous_configuration() {
        let ring = ProductActionDigestKeyringV1::new(key("active", 1), []).unwrap();
        assert!(PostgresProductDecisionsConfig::new(
            ring,
            Duration::from_secs(1),
            Duration::from_secs(1)
        )
        .is_err());
    }

    #[test]
    fn keyring_errors_are_distinct_from_timeout_errors() {
        assert_eq!(
            ProductActionDigestKeyringV1::new(key("same", 1), [key("same", 2)]).unwrap_err(),
            ProductActionDigestKeyringError::InvalidKeyring
        );
    }
}
