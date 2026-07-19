use sqlx::postgres::PgPool;

use super::config::{
    PostgresProductDecisionsConfig, ProductDecisionConfigError, ProductDecisionDigestKeyringV1,
};
use super::digest::keyring_coverage_identity;
use crate::ProductDatabaseFailureV1;

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
        keyring: ProductDecisionDigestKeyringV1,
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

    pub async fn verify_keyring_coverage(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = keyring_coverage_identity(self.config.keyring());
        let mut transaction = self
            .pools
            .approval_executor
            .begin()
            .await
            .map_err(readiness_database)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, TRUE)")
            .bind(self.config.statement_timeout())
            .execute(&mut *transaction)
            .await
            .map_err(readiness_database)?;
        let outcome = sqlx::query_scalar::<_, String>(
            "SELECT outcome FROM public.starring_product_approval_keyring_coverage_v1($1, $2)",
        )
        .bind(&identity.key_ids)
        .bind(&identity.key_fingerprints)
        .fetch_one(&mut *transaction)
        .await
        .map_err(readiness_database)?;
        match outcome.as_str() {
            "ok" => {
                transaction.commit().await.map_err(readiness_database)?;
                Ok(())
            }
            "idempotency_keyring_incomplete" => {
                transaction.rollback().await.map_err(readiness_database)?;
                Err(ProductDecisionReadinessErrorV1::IncompleteCoverage)
            }
            _ => {
                transaction.rollback().await.map_err(readiness_database)?;
                Err(ProductDecisionReadinessErrorV1::InvalidResult)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDecisionReadinessErrorV1 {
    #[error("product decision keyring does not cover live receipts")]
    IncompleteCoverage,
    #[error("product decision readiness returned an invalid result")]
    InvalidResult,
    #[error(transparent)]
    Database(#[from] ProductDatabaseFailureV1),
}

fn readiness_database(error: sqlx::Error) -> ProductDecisionReadinessErrorV1 {
    ProductDatabaseFailureV1::classify(&error).into()
}
