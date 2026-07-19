use sqlx::postgres::PgPool;

use crate::{
    OperatingSystemSecretGenerator, PostgresAuthentication, ProductSecretGenerator,
    ProductSecretGeneratorError, ProductSecretV1,
};

use super::PostgresProductIdentityConfig;

pub(super) const SECRET_INSERT_ATTEMPTS: usize = 4;

#[derive(Clone)]
pub struct PostgresProductIdentityStore<G = OperatingSystemSecretGenerator> {
    pub(super) pool: PgPool,
    pub(super) generator: G,
    pub(super) config: PostgresProductIdentityConfig,
}

impl PostgresProductIdentityStore<OperatingSystemSecretGenerator> {
    pub fn production(pool: PgPool, config: PostgresProductIdentityConfig) -> Self {
        Self::new(pool, OperatingSystemSecretGenerator, config)
    }
}

impl<G> PostgresProductIdentityStore<G> {
    pub fn new(pool: PgPool, generator: G, config: PostgresProductIdentityConfig) -> Self {
        Self {
            pool,
            generator,
            config,
        }
    }

    pub fn authentication(&self) -> PostgresAuthentication {
        PostgresAuthentication::with_config(
            self.pool.clone(),
            self.config.lifetimes().authentication(),
        )
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
