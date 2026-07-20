use authoring_application::AuthorizedDeploymentStatusV1;
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use sqlx::postgres::PgPool;

use crate::ProductDatabaseFailureV1;

use super::super::config::PostgresProductDeploymentStatusesConfig;
use super::contract::STATUS_QUERY;
use super::row::ProductDeploymentOperationalStatusRow;

pub(super) async fn load_status_rows(
    pool: &PgPool,
    config: PostgresProductDeploymentStatusesConfig,
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<Vec<ProductDeploymentOperationalStatusRow>, ProductDatabaseFailureV1> {
    let mut transaction = pool.begin().await.map_err(database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(database)?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $2, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $3, true), \
         pg_catalog.set_config('search_path', 'pg_catalog', true), \
         pg_catalog.set_config('quote_all_identifiers', 'off', true)",
    )
    .bind(config.statement_timeout())
    .bind(config.lock_timeout())
    .bind(config.idle_transaction_timeout())
    .execute(&mut *transaction)
    .await
    .map_err(database)?;
    let scope = request.scope();
    let exact = request.exact_deployment();
    let actor = request.actor();
    let rows = sqlx::query_as::<_, ProductDeploymentOperationalStatusRow>(STATUS_QUERY)
        .bind(exact.deployment_reference())
        .bind(exact.promotion_id().as_str())
        .bind(exact.target_digest())
        .bind(scope.tenant_id().as_str())
        .bind(scope.installation_id().as_str())
        .bind(scope.guild_id().to_string())
        .bind(actor.principal_id().as_str())
        .bind(scope.acting_user_id().to_string())
        .bind(actor.session_fingerprint().as_bytes().as_slice())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database)?;
    transaction.commit().await.map_err(database)?;
    Ok(rows)
}

fn database(error: sqlx::Error) -> ProductDatabaseFailureV1 {
    ProductDatabaseFailureV1::classify(&error)
}
