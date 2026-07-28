use super::ProductControlApplication;
use crate::status::validate_decision_projection;
use crate::{
    AuthenticationPort, AuthorizedCancelProductLifecycleV1, CancelProductLifecycleMutationV1,
    CapabilityV1, FreshGuildAuthorityPort, InstallationSelectorV1, MutationAuthenticationPort,
    ProductApplicationError, ProductDecisionPhaseV1, ProductLifecycleCancellationPort,
    ProductLifecycleCancellationReceiptV1, ProductRequestIdV1,
};

impl<A, G, D, R> ProductControlApplication<'_, A, G, D, R>
where
    A: AuthenticationPort,
    G: FreshGuildAuthorityPort,
{
    pub async fn cancel_product_lifecycle(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: CancelProductLifecycleMutationV1,
    ) -> Result<ProductLifecycleCancellationReceiptV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
        D: ProductLifecycleCancellationPort<G::Evidence>,
    {
        let promotion = command.promotion.clone();
        let expected_revision = command.expected_revision;
        let expected_drain_selector = command.drain_selector.clone();
        let (actor, authorized) = self
            .authenticate_mutation_and_authorize(
                credential,
                csrf,
                installation,
                CapabilityV1::CancelLifecycle,
            )
            .await?;
        let receipt = self
            .decisions
            .cancel_lifecycle_idempotent(AuthorizedCancelProductLifecycleV1::new(
                request_id,
                &actor,
                authorized.scope(),
                authorized.evidence(),
                command,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &promotion, receipt.decision())?;
        let expected_terminal_intent_revision = expected_drain_selector
            .acknowledged_intent_revision()
            .get()
            .checked_add(1);
        let expected_resulting_deployment_revision = expected_drain_selector
            .expected_runtime_deployment_revision()
            .get()
            .checked_add(1);
        let expected_successor_epoch = receipt.source_slot_writer_epoch().get().checked_add(1);
        if receipt.decision().revision() != expected_revision
            || receipt.decision().phase() != &ProductDecisionPhaseV1::Approved
            || receipt.source_drain_selector() != &expected_drain_selector
            || expected_terminal_intent_revision != Some(receipt.terminal_intent_revision().get())
            || expected_resulting_deployment_revision
                != Some(receipt.resulting_runtime_deployment_revision().get())
            || expected_successor_epoch != Some(receipt.successor_slot_writer_epoch().get())
        {
            return Err(ProductApplicationError::InvalidProjection);
        }
        Ok(receipt)
    }
}
