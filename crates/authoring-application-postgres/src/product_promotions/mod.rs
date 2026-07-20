#[allow(dead_code)]
mod activation_link;
#[allow(dead_code)]
mod admission;
#[allow(dead_code)]
mod approval_environment;
#[allow(dead_code)]
mod authorization;
mod config;
#[allow(dead_code)]
mod digest;
mod orchestrator;
#[allow(dead_code)]
mod prepare;
#[allow(dead_code)]
mod publication;
mod readiness;
#[allow(dead_code)]
mod repair;
#[allow(dead_code)]
mod replay;
#[allow(dead_code)]
mod row;
mod store;
#[allow(dead_code)]
mod transaction;

pub use config::{PostgresProductPromotionsConfig, ProductPromotionConfigError};
pub use readiness::ProductPromotionReadinessErrorV1;
pub use store::PostgresProductPromotions;
