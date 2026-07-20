use sqlx::postgres::PgPool;

#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicU64};
#[cfg(test)]
use std::sync::Arc;

use crate::{
    OperatingSystemSecretGenerator, PostgresAuthentication, ProductSecretGenerator,
    ProductSecretGeneratorError, ProductSecretV1,
};

use super::PostgresProductIdentityConfig;

pub(super) const SECRET_INSERT_ATTEMPTS: usize = 4;

#[derive(Clone)]
pub struct ProductIdentityDatabasePoolsV1 {
    pub(super) oauth_flow_writer: PgPool,
    pub(super) session_issuer: PgPool,
    pub(super) session_api: PgPool,
    pub(super) security_revoker: PgPool,
}

impl ProductIdentityDatabasePoolsV1 {
    pub fn new(
        oauth_flow_writer: PgPool,
        session_issuer: PgPool,
        session_api: PgPool,
        security_revoker: PgPool,
    ) -> Self {
        Self {
            oauth_flow_writer,
            session_issuer,
            session_api,
            security_revoker,
        }
    }
}

#[derive(Clone)]
pub struct PostgresProductIdentityStore<G = OperatingSystemSecretGenerator> {
    pub(super) pools: ProductIdentityDatabasePoolsV1,
    pub(super) generator: G,
    pub(super) config: PostgresProductIdentityConfig,
    #[cfg(test)]
    pub(super) session_issue_commit_ack_loss_delay_millis: Arc<AtomicU64>,
    #[cfg(test)]
    pub(super) session_issue_close_pool_after_ack_loss: Arc<AtomicBool>,
    #[cfg(test)]
    pub(super) session_issue_rollback_before_ack_loss: Arc<AtomicBool>,
}

impl PostgresProductIdentityStore<OperatingSystemSecretGenerator> {
    pub fn production(
        pools: ProductIdentityDatabasePoolsV1,
        config: PostgresProductIdentityConfig,
    ) -> Self {
        Self::new(pools, OperatingSystemSecretGenerator, config)
    }
}

impl<G> PostgresProductIdentityStore<G> {
    pub fn new(
        pools: ProductIdentityDatabasePoolsV1,
        generator: G,
        config: PostgresProductIdentityConfig,
    ) -> Self {
        Self {
            pools,
            generator,
            config,
            #[cfg(test)]
            session_issue_commit_ack_loss_delay_millis: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            session_issue_close_pool_after_ack_loss: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            session_issue_rollback_before_ack_loss: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn authentication(&self) -> PostgresAuthentication {
        PostgresAuthentication::with_config(
            self.pools.session_api.clone(),
            self.config.lifetimes().authentication(),
        )
    }

    pub fn oauth_redirect_uri(&self) -> &str {
        self.config.redirect_uri()
    }

    pub fn allows_return_path(&self, return_path: &str) -> bool {
        self.config.allows_return_path(return_path)
    }
}

impl<G> PostgresProductIdentityStore<G>
where
    G: ProductSecretGenerator,
{
    pub(super) fn generate_distinct_pair(
        &self,
    ) -> Result<(ProductSecretV1, ProductSecretV1), ProductSecretGeneratorError> {
        for _ in 0..SECRET_INSERT_ATTEMPTS {
            let first = ProductSecretV1::generate(&self.generator)?;
            let second = ProductSecretV1::generate(&self.generator)?;
            if first.expose_secret() != second.expose_secret() {
                return Ok((first, second));
            }
        }
        Err(ProductSecretGeneratorError::Unavailable)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
