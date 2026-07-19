use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::Duration;

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MAX_DIGEST_KEYS: usize = 8;
const MAX_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDecisionDigestKeyError {
    #[error("product decision digest key ID is invalid")]
    InvalidKeyId,
    #[error("product decision digest key material is not production-safe")]
    WeakKeyMaterial,
}

pub struct ProductDecisionDigestKeyV1 {
    key_id: String,
    secret: Zeroizing<[u8; 32]>,
}

impl ProductDecisionDigestKeyV1 {
    pub fn from_bytes(
        key_id: &str,
        secret: [u8; 32],
    ) -> Result<Self, ProductDecisionDigestKeyError> {
        if key_id.is_empty()
            || key_id.len() > 64
            || !key_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(ProductDecisionDigestKeyError::InvalidKeyId);
        }
        if obvious_repetition(&secret) {
            return Err(ProductDecisionDigestKeyError::WeakKeyMaterial);
        }
        Ok(Self {
            key_id: key_id.to_string(),
            secret: Zeroizing::new(secret),
        })
    }

    pub(crate) fn key_id(&self) -> &str {
        &self.key_id
    }

    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl Debug for ProductDecisionDigestKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDecisionDigestKeyV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDecisionConfigError {
    #[error("product decision digest keyring must contain one to eight unique keys")]
    InvalidKeyring,
    #[error("product decision statement timeout is invalid")]
    InvalidStatementTimeout,
    #[error("product decision lock timeout is invalid")]
    InvalidLockTimeout,
}

#[derive(Clone)]
pub struct ProductDecisionDigestKeyringV1 {
    keys: Arc<[ProductDecisionDigestKeyV1]>,
}

impl ProductDecisionDigestKeyringV1 {
    pub fn new(
        active: ProductDecisionDigestKeyV1,
        retired: impl IntoIterator<Item = ProductDecisionDigestKeyV1>,
    ) -> Result<Self, ProductDecisionConfigError> {
        let keys = std::iter::once(active).chain(retired).collect::<Vec<_>>();
        let unique_ids = keys
            .iter()
            .map(ProductDecisionDigestKeyV1::key_id)
            .collect::<BTreeSet<_>>();
        let duplicate_material = keys.iter().enumerate().any(|(index, candidate)| {
            keys.iter()
                .skip(index + 1)
                .any(|other| bool::from(candidate.secret().ct_eq(other.secret())))
        });
        if keys.is_empty()
            || keys.len() > MAX_DIGEST_KEYS
            || unique_ids.len() != keys.len()
            || duplicate_material
        {
            return Err(ProductDecisionConfigError::InvalidKeyring);
        }
        Ok(Self { keys: keys.into() })
    }

    pub(crate) fn active(&self) -> &ProductDecisionDigestKeyV1 {
        &self.keys[0]
    }

    pub(crate) fn keys(&self) -> &[ProductDecisionDigestKeyV1] {
        &self.keys
    }
}

fn obvious_repetition(secret: &[u8; 32]) -> bool {
    [1_usize, 2, 4, 8, 16]
        .into_iter()
        .any(|period| (period..secret.len()).all(|index| secret[index] == secret[index % period]))
}

impl Debug for ProductDecisionDigestKeyringV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDecisionDigestKeyringV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct PostgresProductDecisionsConfig {
    keyring: ProductDecisionDigestKeyringV1,
    statement_timeout: Duration,
    lock_timeout: Duration,
}

impl PostgresProductDecisionsConfig {
    pub fn new(
        keyring: ProductDecisionDigestKeyringV1,
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
        keyring: ProductDecisionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Self::new(keyring, Duration::from_secs(2), Duration::from_millis(500))
    }

    pub(crate) fn keyring(&self) -> &ProductDecisionDigestKeyringV1 {
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

    fn key(id: &str, byte: u8) -> ProductDecisionDigestKeyV1 {
        ProductDecisionDigestKeyV1::from_bytes(
            id,
            std::array::from_fn(|index| byte.wrapping_add(index as u8)),
        )
        .unwrap()
    }

    #[test]
    fn keyring_preserves_active_then_retired_order_without_exposing_secrets() {
        let ring =
            ProductDecisionDigestKeyringV1::new(key("active-v2", 2), [key("old-v1", 1)]).unwrap();
        assert_eq!(ring.active().key_id(), "active-v2");
        assert_eq!(ring.keys()[1].key_id(), "old-v1");
        assert_eq!(
            format!("{ring:?}"),
            "ProductDecisionDigestKeyringV1(<redacted>)"
        );
        assert!(!format!("{:?}", ring.active()).contains("020202"));
    }

    #[test]
    fn keyring_and_timeouts_reject_ambiguous_configuration() {
        assert!(ProductDecisionDigestKeyV1::from_bytes("bad key", [1; 32]).is_err());
        assert!(ProductDecisionDigestKeyringV1::new(key("same", 1), [key("same", 2)]).is_err());
        let ring = ProductDecisionDigestKeyringV1::new(key("active", 1), []).unwrap();
        assert!(PostgresProductDecisionsConfig::new(
            ring,
            Duration::from_secs(1),
            Duration::from_secs(1)
        )
        .is_err());
    }

    #[test]
    fn keyring_rejects_repeated_or_aliased_secret_material() {
        assert_eq!(
            ProductDecisionDigestKeyV1::from_bytes("zero", [0; 32]).unwrap_err(),
            ProductDecisionDigestKeyError::WeakKeyMaterial
        );
        assert_eq!(
            ProductDecisionDigestKeyV1::from_bytes("repeat", [7; 32]).unwrap_err(),
            ProductDecisionDigestKeyError::WeakKeyMaterial
        );
        let material = std::array::from_fn(|index| index as u8);
        let first = ProductDecisionDigestKeyV1::from_bytes("first", material).unwrap();
        let second = ProductDecisionDigestKeyV1::from_bytes("second", material).unwrap();
        assert_eq!(
            ProductDecisionDigestKeyringV1::new(first, [second]).unwrap_err(),
            ProductDecisionConfigError::InvalidKeyring
        );
    }
}
