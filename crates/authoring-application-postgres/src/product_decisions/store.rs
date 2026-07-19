use sqlx::postgres::PgPool;

use super::config::{
    PostgresProductDecisionsConfig, ProductDecisionConfigError, ProductDecisionDigestKeyringV1,
};
use super::digest::keyring_coverage_identity;
use crate::ProductDatabaseFailureV1;

#[derive(Clone)]
pub struct PostgresProductDecisions {
    pub(super) pool: PgPool,
    pub(super) config: PostgresProductDecisionsConfig,
}

impl PostgresProductDecisions {
    pub fn new(
        pool: PgPool,
        keyring: ProductDecisionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Ok(Self {
            pool,
            config: PostgresProductDecisionsConfig::production(keyring)?,
        })
    }

    pub fn with_config(pool: PgPool, config: PostgresProductDecisionsConfig) -> Self {
        Self { pool, config }
    }

    pub async fn verify_keyring_coverage(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        let identity = keyring_coverage_identity(self.config.keyring());
        let mut transaction = self.pool.begin().await.map_err(readiness_database)?;
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
