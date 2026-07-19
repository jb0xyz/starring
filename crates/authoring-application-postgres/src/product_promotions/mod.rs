#[allow(dead_code)]
mod admission;
#[allow(dead_code)]
mod authorization;
mod config;
#[allow(dead_code)]
mod digest;
mod store;

pub use config::{PostgresProductPromotionsConfig, ProductPromotionConfigError};
pub use store::PostgresProductPromotions;
