use std::fmt::{Debug, Formatter};

use sqlx::postgres::PgPool;

use crate::product_action_digest::ProductActionDigestKeyringV1;

use super::config::{PostgresProductPromotionsConfig, ProductPromotionConfigError};

#[derive(Clone)]
pub struct PostgresProductPromotions {
    #[allow(dead_code)]
    pub(super) executor: PgPool,
    #[allow(dead_code)]
    pub(super) config: PostgresProductPromotionsConfig,
}

impl PostgresProductPromotions {
    pub fn new(
        executor: PgPool,
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductPromotionConfigError> {
        Ok(Self {
            executor,
            config: PostgresProductPromotionsConfig::production(keyring)?,
        })
    }

    pub fn with_config(executor: PgPool, config: PostgresProductPromotionsConfig) -> Self {
        Self { executor, config }
    }
}

impl Debug for PostgresProductPromotions {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PostgresProductPromotions(<redacted>)")
    }
}
