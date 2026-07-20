use super::projection_validation::validate_approval_phase;
use super::ProductControlApplication;
use crate::status::validate_decision_projection;
use crate::{
    ApplyProductPromotionV1, ApproveProductPromotionV1, AuthenticationPort,
    AuthorizedApplyProductV1, AuthorizedApproveProductV1, AuthorizedRejectProductV1, CapabilityV1,
    DeploymentStatusPort, FreshGuildAuthorityPort, InstallationSelectorV1,
    MutationAuthenticationPort, ProductApplicationError, ProductApplyPort, ProductApplyResultV1,
    ProductApprovalPort, ProductControlPortError, ProductDecisionPhaseV1, ProductMutationReceiptV1,
    ProductRejectionPort, ProductRequestIdV1, ProductStatusV1, RejectProductPromotionV1,
};

impl<A, G, D, R> ProductControlApplication<'_, A, G, D, R>
where
    A: AuthenticationPort,
    G: FreshGuildAuthorityPort,
{
    pub async fn approve(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: ApproveProductPromotionV1,
    ) -> Result<ProductMutationReceiptV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
        D: ProductApprovalPort<G::Evidence>,
    {
        let promotion = command.promotion.clone();
        let (actor, authorized) = self
            .authenticate_mutation_and_authorize(
                credential,
                csrf,
                installation,
                CapabilityV1::Approve,
            )
            .await?;
        let receipt = self
            .decisions
            .approve_payload_bound(AuthorizedApproveProductV1::new(
                request_id,
                &actor,
                authorized.scope(),
                authorized.evidence(),
                command,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &promotion, receipt.projection())?;
        validate_approval_phase(receipt.projection().phase())?;
        Ok(receipt)
    }

    pub async fn reject(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: RejectProductPromotionV1,
    ) -> Result<ProductMutationReceiptV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
        D: ProductRejectionPort<G::Evidence>,
    {
        let promotion = command.promotion.clone();
        let (actor, authorized) = self
            .authenticate_mutation_and_authorize(
                credential,
                csrf,
                installation,
                CapabilityV1::Reject,
            )
            .await?;
        let receipt = self
            .decisions
            .reject_payload_bound(AuthorizedRejectProductV1::new(
                request_id,
                &actor,
                authorized.scope(),
                authorized.evidence(),
                command,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &promotion, receipt.projection())?;
        Ok(receipt)
    }

    pub async fn apply(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: ApplyProductPromotionV1,
    ) -> Result<ProductApplyResultV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
        D: ProductApplyPort<G::Evidence>,
        R: DeploymentStatusPort<G::Evidence>,
    {
        let promotion = command.promotion.clone();
        let (actor, authorized) = self
            .authenticate_mutation_and_authorize(
                credential,
                csrf,
                installation,
                CapabilityV1::Apply,
            )
            .await?;
        let receipt = self
            .decisions
            .apply_idempotent(AuthorizedApplyProductV1::new(
                request_id,
                &actor,
                authorized.scope(),
                authorized.evidence(),
                command,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &promotion, receipt.projection())?;
        let exact_deployment = match receipt.projection().phase() {
            ProductDecisionPhaseV1::Applied { exact_deployment } => exact_deployment,
            ProductDecisionPhaseV1::Superseded => {
                return Err(ProductControlPortError::Superseded.into());
            }
            _ => return Err(ProductApplicationError::InvalidProjection),
        };
        let status = if receipt.exact_replay() {
            self.resolve_product_status(&actor, &authorized, receipt.projection())
                .await?
        } else {
            ProductStatusV1::RuntimePending
        };
        Ok(ProductApplyResultV1::from_verified_application(
            status,
            receipt.exact_replay(),
            exact_deployment.clone(),
        ))
    }
}
