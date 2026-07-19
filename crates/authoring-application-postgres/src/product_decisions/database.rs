use authoring_application::ProductControlPortError;

use super::config::PostgresProductDecisionsConfig;
use crate::ProductDatabaseFailureV1;

pub(super) async fn configure_mutation_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &PostgresProductDecisionsConfig,
) -> Result<(), ProductControlPortError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **transaction)
        .await
        .map_err(database_backend)?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(config.statement_timeout())
        .execute(&mut **transaction)
        .await
        .map_err(database_backend)?;
    sqlx::query("SELECT pg_catalog.set_config('lock_timeout', $1, true)")
        .bind(config.lock_timeout())
        .execute(&mut **transaction)
        .await
        .map_err(database_backend)?;
    Ok(())
}

pub(super) fn database_backend(error: sqlx::Error) -> ProductControlPortError {
    ProductControlPortError::Backend(ProductDatabaseFailureV1::classify(&error).to_string())
}

pub(super) fn database_commit(
    error: sqlx::Error,
    operation: &'static str,
) -> ProductControlPortError {
    if matches!(&error, sqlx::Error::Database(_)) {
        database_backend(error)
    } else {
        ProductControlPortError::Indeterminate(operation.to_string())
    }
}
