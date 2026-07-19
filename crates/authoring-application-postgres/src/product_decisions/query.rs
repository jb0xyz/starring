use authoring_application::{
    AuthorizedApprovalPreviewV1, AuthorizedProductStatusV1, CapabilityV1, ProductApprovalPreviewV1,
    ProductControlPortError, ProductDecisionProjectionV1, ProductDecisionQueryPort,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use super::database::{configure_read_transaction, database_backend};
use super::reader_contract::READ_QUERY;
use super::row::{invalid_persistence, validate_decision_row, ProductDecisionRow};
use super::store::PostgresProductDecisions;

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDecisions {
    async fn load_approval_preview(
        &self,
        request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        let validated = self
            .load_validated(
                request.actor(),
                request.scope(),
                request.evidence(),
                request.promotion(),
            )
            .await?;
        Ok(validated.preview)
    }

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        let validated = self
            .load_validated(
                request.actor(),
                request.scope(),
                request.evidence(),
                request.promotion(),
            )
            .await?;
        Ok(validated.projection)
    }
}

impl PostgresProductDecisions {
    async fn load_validated(
        &self,
        actor: &authoring_application::AuthenticatedActorV1,
        scope: &authoring_application::AuthorizedInstallationScopeV1,
        evidence: &FreshDiscordAuthorityEvidenceV1,
        promotion: &authoring_application::PromotionSelectorV1,
    ) -> Result<super::row::ValidatedDecisionRow, ProductControlPortError> {
        let mut transaction = self
            .pools
            .decision_reader
            .begin()
            .await
            .map_err(database_backend)?;
        configure_read_transaction(&mut transaction, &self.config).await?;
        let mut rows = sqlx::query_as::<_, ProductDecisionRow>(READ_QUERY)
            .bind(promotion.promotion_id().as_str())
            .bind(scope.tenant_id().as_str())
            .bind(scope.installation_id().as_str())
            .bind(scope.guild_id().to_string())
            .bind(actor.principal_id().as_str())
            .bind(scope.acting_user_id().to_string())
            .bind(actor.session_fingerprint().as_bytes().as_slice())
            .fetch_all(&mut *transaction)
            .await
            .map_err(database_backend)?;
        let row = match rows.len() {
            0 => return Err(ProductControlPortError::NotFound),
            1 => rows.pop().ok_or_else(invalid_persistence)?,
            _ => return Err(invalid_persistence()),
        };
        let validated = validate_decision_row(row, scope, evidence, promotion, CapabilityV1::Read)?;
        transaction.commit().await.map_err(database_backend)?;
        Ok(validated)
    }
}
