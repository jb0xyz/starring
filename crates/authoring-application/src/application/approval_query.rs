use super::projection_validation::{validate_preview, validate_preview_observation};
use super::ProductControlApplication;
use crate::{
    AuthenticationPort, AuthorizedApprovalPreviewV1, CapabilityV1, FreshGuildAuthorityPort,
    InstallationSelectorV1, ProductApplicationError, ProductApprovalPreviewObservationV1,
    ProductApprovalPreviewV1, ProductDecisionObservationPort, ProductDecisionQueryPort,
    ProductStatusQueryV1,
};

impl<A, G, D, R> ProductControlApplication<'_, A, G, D, R>
where
    A: AuthenticationPort,
    G: FreshGuildAuthorityPort,
{
    pub async fn get_approval_preview(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: ProductStatusQueryV1,
    ) -> Result<ProductApprovalPreviewV1, ProductApplicationError>
    where
        D: ProductDecisionQueryPort<G::Evidence>,
    {
        let (actor, authorized) = self
            .authenticate_and_authorize(credential, installation, CapabilityV1::Read)
            .await?;
        let preview = self
            .decisions
            .load_approval_preview(AuthorizedApprovalPreviewV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &query.promotion,
            ))
            .await?;
        validate_preview(authorized.scope(), &query.promotion, &preview)?;
        Ok(preview)
    }

    pub async fn get_approval_preview_observation(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: ProductStatusQueryV1,
    ) -> Result<ProductApprovalPreviewObservationV1, ProductApplicationError>
    where
        D: ProductDecisionObservationPort<G::Evidence>,
    {
        let (actor, authorized) = self
            .authenticate_and_authorize(credential, installation, CapabilityV1::Read)
            .await?;
        let observation = self
            .decisions
            .load_approval_preview_observation(AuthorizedApprovalPreviewV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &query.promotion,
            ))
            .await?;
        validate_preview_observation(authorized.scope(), &query.promotion, &observation)?;
        Ok(observation)
    }
}
