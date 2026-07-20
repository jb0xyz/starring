use sqlx::postgres::PgPool;

use crate::product_action_digest::ProductActionDigestKeyringV1;

use super::config::{PostgresProductDecisionsConfig, ProductDecisionConfigError};

#[derive(Clone)]
pub struct ProductDecisionDatabasePoolsV1 {
    pub(super) decision_reader: PgPool,
    pub(super) approval_executor: PgPool,
    pub(super) apply_executor: PgPool,
}

impl ProductDecisionDatabasePoolsV1 {
    pub fn new(decision_reader: PgPool, approval_executor: PgPool, apply_executor: PgPool) -> Self {
        Self {
            decision_reader,
            approval_executor,
            apply_executor,
        }
    }
}

#[derive(Clone)]
pub struct PostgresProductDecisions {
    pub(super) pools: ProductDecisionDatabasePoolsV1,
    pub(super) config: PostgresProductDecisionsConfig,
}

impl PostgresProductDecisions {
    pub fn new(
        pools: ProductDecisionDatabasePoolsV1,
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Ok(Self {
            pools,
            config: PostgresProductDecisionsConfig::production(keyring)?,
        })
    }

    pub fn with_config(
        pools: ProductDecisionDatabasePoolsV1,
        config: PostgresProductDecisionsConfig,
    ) -> Self {
        Self { pools, config }
    }
}
