use authoring_application::{
    AuthorizedApplyProductV1, AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1,
    AuthorizedCancelProductLifecycleV1, AuthorizedProductStatusV1, AuthorizedRejectProductV1,
    ProductApplyPort, ProductApprovalPort, ProductApprovalPreviewObservationV1,
    ProductApprovalPreviewV1, ProductControlPortError, ProductDecisionObservationPort,
    ProductDecisionObservationV1, ProductDecisionProjectionV1, ProductDecisionQueryPort,
    ProductLifecycleCancellationPort, ProductLifecycleCancellationReceiptV1,
    ProductMutationReceiptV1, ProductRejectionPort,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use sqlx::postgres::PgPool;

use crate::database_capability::{verify_same_database_distinct_roles, ScopedDatabaseTopologyV1};
use crate::product_action_digest::ProductActionDigestKeyringV1;

use super::config::{PostgresProductDecisionsConfig, ProductDecisionConfigError};
use super::lifecycle_cancel::PostgresProductLifecycleCancellations;
use super::readiness::{map_readiness, ProductDecisionReadinessErrorV1};
use super::reject::PostgresProductRejections;
use super::store::{PostgresProductDecisions, ProductDecisionDatabasePoolsV1};

#[derive(Clone)]
pub struct PostgresProductControl {
    decisions: PostgresProductDecisions,
    rejections: PostgresProductRejections,
    cancellations: PostgresProductLifecycleCancellations,
}

impl PostgresProductControl {
    pub fn new(
        decision_pools: ProductDecisionDatabasePoolsV1,
        rejection_executor: PgPool,
        cancellation_executor: PgPool,
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Ok(Self::with_config(
            decision_pools,
            rejection_executor,
            cancellation_executor,
            PostgresProductDecisionsConfig::production(keyring)?,
        ))
    }

    pub fn with_config(
        decision_pools: ProductDecisionDatabasePoolsV1,
        rejection_executor: PgPool,
        cancellation_executor: PgPool,
        config: PostgresProductDecisionsConfig,
    ) -> Self {
        Self {
            decisions: PostgresProductDecisions::with_config(decision_pools, config.clone()),
            rejections: PostgresProductRejections::with_config(rejection_executor, config.clone()),
            cancellations: PostgresProductLifecycleCancellations::with_config(
                cancellation_executor,
                config,
            ),
        }
    }

    pub async fn verify_readiness(&self) -> Result<(), ProductDecisionReadinessErrorV1> {
        self.check_readiness().await.map(drop)
    }

    pub(crate) async fn check_readiness(
        &self,
    ) -> Result<[ScopedDatabaseTopologyV1; 5], ProductDecisionReadinessErrorV1> {
        let topologies = [
            self.decisions.check_decision_reader_readiness().await?,
            self.decisions.check_approval_executor_readiness().await?,
            self.rejections.check_product_rejection_readiness().await?,
            self.decisions.check_apply_executor_readiness().await?,
            self.cancellations
                .check_product_lifecycle_cancellation_readiness()
                .await?,
        ];
        verify_same_database_distinct_roles(&topologies).map_err(map_readiness)?;
        Ok(topologies)
    }
}

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductControl {
    async fn load_approval_preview(
        &self,
        request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        ProductDecisionQueryPort::load_approval_preview(&self.decisions, request).await
    }

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        ProductDecisionQueryPort::load_product_status(&self.decisions, request).await
    }
}

impl ProductDecisionObservationPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductControl {
    async fn load_approval_preview_observation(
        &self,
        request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewObservationV1, ProductControlPortError> {
        ProductDecisionObservationPort::load_approval_preview_observation(&self.decisions, request)
            .await
    }

    async fn load_product_status_observation(
        &self,
        request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionObservationV1, ProductControlPortError> {
        ProductDecisionObservationPort::load_product_status_observation(&self.decisions, request)
            .await
    }
}

impl ProductApprovalPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductControl {
    async fn approve_payload_bound(
        &self,
        request: AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        ProductApprovalPort::approve_payload_bound(&self.decisions, request).await
    }
}

impl ProductRejectionPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductControl {
    async fn reject_payload_bound(
        &self,
        request: AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        ProductRejectionPort::reject_payload_bound(&self.rejections, request).await
    }
}

impl ProductApplyPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductControl {
    async fn apply_idempotent(
        &self,
        request: AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        ProductApplyPort::apply_idempotent(&self.decisions, request).await
    }
}

impl ProductLifecycleCancellationPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductControl {
    async fn cancel_lifecycle_idempotent(
        &self,
        request: AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductLifecycleCancellationReceiptV1, ProductControlPortError> {
        ProductLifecycleCancellationPort::cancel_lifecycle_idempotent(&self.cancellations, request)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use authoring_application::ProductDecisionPort;
    use sqlx::postgres::PgPoolOptions;

    use super::*;
    use crate::product_action_digest::ProductActionDigestKeyV1;

    fn keyring() -> ProductActionDigestKeyringV1 {
        ProductActionDigestKeyringV1::new(
            ProductActionDigestKeyV1::from_bytes(
                "facade-active",
                std::array::from_fn(|index| 19_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
            [],
        )
        .unwrap()
    }

    fn pool(database: &str) -> PgPool {
        PgPoolOptions::new()
            .connect_lazy(&format!("postgresql://localhost/{database}"))
            .unwrap()
    }

    fn assert_product_decision_port<T: ProductDecisionPort<FreshDiscordAuthorityEvidenceV1>>() {}

    #[test]
    fn facade_satisfies_the_complete_product_decision_port() {
        assert_product_decision_port::<PostgresProductControl>();
    }

    #[tokio::test]
    async fn facade_reuses_one_validated_configuration_across_mutation_adapters() {
        let config = PostgresProductDecisionsConfig::new(
            keyring(),
            Duration::from_secs(3),
            Duration::from_millis(700),
        )
        .unwrap();
        let facade = PostgresProductControl::with_config(
            ProductDecisionDatabasePoolsV1::new(
                pool("facade_reader"),
                pool("facade_approval"),
                pool("facade_apply"),
            ),
            pool("facade_rejection"),
            pool("facade_cancellation"),
            config,
        );

        assert_eq!(
            facade.decisions.config.statement_timeout(),
            facade.rejections.config.statement_timeout()
        );
        assert_eq!(
            facade.decisions.config.lock_timeout(),
            facade.rejections.config.lock_timeout()
        );
        assert_eq!(
            facade.decisions.config.keyring().active().key_id(),
            facade.rejections.config.keyring().active().key_id()
        );
        assert_eq!(
            facade.decisions.config.keyring().active().key_id(),
            facade.cancellations.config.keyring().active().key_id()
        );
    }
}
