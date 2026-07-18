use authoring_promotion::approval_payload_digest_v1;

use crate::authority::validate_authorized_scope;
use crate::promotion::build_start_promotion;
use crate::status::{map_non_applied_status, validate_decision_projection, validate_exact_live};
use crate::{
    ApplyProductPromotionV1, AuthenticatedActorV1, AuthenticationPort, AuthorizedApplyProductV1,
    AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1, AuthorizedDeploymentStatusV1,
    AuthorizedInstallationV1, AuthorizedProductStatusV1, AuthorizedPromotionSnapshotPort,
    AuthorizedRejectProductV1, CapabilityV1, DeploymentStatusPort, DeploymentStatusProjectionV1,
    DeploymentStatusV1, FreshGuildAuthorityPort, InstallationSelectorV1,
    MutationAuthenticationPort, ProductApplicationError, ProductApprovalPreviewV1,
    ProductDecisionPhaseV1, ProductDecisionPort, ProductDecisionProjectionV1,
    ProductMutationReceiptV1, ProductStatusQueryV1, ProductStatusV1, PromoteOwnedSessionV1,
    PromotionSubmissionPort, RejectProductPromotionV1, RuntimeDeploymentQueryV1,
};
use crate::{ApproveProductPromotionV1, AuthoringApplicationError};

pub struct AuthoringApplication<'a, A, G, S, P> {
    authentication: &'a A,
    guild_authority: &'a G,
    snapshots: &'a S,
    promotions: &'a P,
}

impl<'a, A, G, S, P> AuthoringApplication<'a, A, G, S, P> {
    pub fn new(
        authentication: &'a A,
        guild_authority: &'a G,
        snapshots: &'a S,
        promotions: &'a P,
    ) -> Self {
        Self {
            authentication,
            guild_authority,
            snapshots,
            promotions,
        }
    }
}

impl<A, G, S, P> AuthoringApplication<'_, A, G, S, P>
where
    A: MutationAuthenticationPort,
    G: FreshGuildAuthorityPort,
    S: AuthorizedPromotionSnapshotPort<G::Evidence>,
    P: PromotionSubmissionPort,
{
    pub async fn promote_owned_session(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
    ) -> Result<P::Output, AuthoringApplicationError> {
        let claims = self
            .authentication
            .authenticate_mutation(credential, csrf)
            .await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, CapabilityV1::Promote)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        let snapshot = self
            .snapshots
            .load_atomic_authorized_snapshot(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &command.session_id,
                command.expected_generation,
            )
            .await?;
        let input = build_start_promotion(&actor, authorized.scope(), command, snapshot)?;
        self.promotions
            .submit_verified_promotion(input)
            .await
            .map_err(AuthoringApplicationError::Promotion)
    }
}

pub struct ProductControlApplication<'a, A, G, D, R> {
    authentication: &'a A,
    guild_authority: &'a G,
    decisions: &'a D,
    deployments: &'a R,
}

impl<'a, A, G, D, R> ProductControlApplication<'a, A, G, D, R> {
    pub fn new(
        authentication: &'a A,
        guild_authority: &'a G,
        decisions: &'a D,
        deployments: &'a R,
    ) -> Self {
        Self {
            authentication,
            guild_authority,
            decisions,
            deployments,
        }
    }
}

impl<A, G, D, R> ProductControlApplication<'_, A, G, D, R>
where
    A: AuthenticationPort,
    G: FreshGuildAuthorityPort,
    D: ProductDecisionPort<G::Evidence>,
    R: DeploymentStatusPort<G::Evidence>,
{
    async fn authenticate_and_authorize(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<
        (AuthenticatedActorV1, AuthorizedInstallationV1<G::Evidence>),
        ProductApplicationError,
    > {
        let claims = self.authentication.authenticate(credential).await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, capability)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        Ok((actor, authorized))
    }

    async fn authenticate_mutation_and_authorize(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<
        (AuthenticatedActorV1, AuthorizedInstallationV1<G::Evidence>),
        ProductApplicationError,
    >
    where
        A: MutationAuthenticationPort,
    {
        let claims = self
            .authentication
            .authenticate_mutation(credential, csrf)
            .await?;
        let actor = AuthenticatedActorV1::from_authentication_claims(claims);
        let authorized = self
            .guild_authority
            .authorize_installation(&actor, installation, capability)
            .await?;
        validate_authorized_scope(installation, authorized.scope())?;
        Ok((actor, authorized))
    }

    pub async fn get_approval_preview(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: ProductStatusQueryV1,
    ) -> Result<ProductApprovalPreviewV1, ProductApplicationError> {
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

    pub async fn get_product_status(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: ProductStatusQueryV1,
    ) -> Result<ProductStatusV1, ProductApplicationError> {
        let (actor, authorized) = self
            .authenticate_and_authorize(credential, installation, CapabilityV1::Read)
            .await?;
        let projection = self
            .decisions
            .load_product_status(AuthorizedProductStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &query.promotion,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &query.promotion, &projection)?;
        self.resolve_product_status(&actor, &authorized, &projection)
            .await
    }

    pub async fn approve(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
        command: ApproveProductPromotionV1,
    ) -> Result<ProductMutationReceiptV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
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
                &actor,
                authorized.scope(),
                authorized.evidence(),
                command,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &promotion, receipt.projection())?;
        Ok(receipt)
    }

    pub async fn reject(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        installation: &InstallationSelectorV1,
        command: RejectProductPromotionV1,
    ) -> Result<ProductMutationReceiptV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
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
        installation: &InstallationSelectorV1,
        command: ApplyProductPromotionV1,
    ) -> Result<ProductStatusV1, ProductApplicationError>
    where
        A: MutationAuthenticationPort,
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
                &actor,
                authorized.scope(),
                authorized.evidence(),
                command,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &promotion, receipt.projection())?;
        self.resolve_product_status(&actor, &authorized, receipt.projection())
            .await
    }

    pub async fn get_deployment_status(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: RuntimeDeploymentQueryV1,
    ) -> Result<DeploymentStatusV1, ProductApplicationError> {
        let (actor, authorized) = self
            .authenticate_and_authorize(credential, installation, CapabilityV1::Read)
            .await?;
        let projection = self
            .decisions
            .load_product_status(AuthorizedProductStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &query.promotion,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &query.promotion, &projection)?;
        let ProductDecisionPhaseV1::Applied { exact_deployment } = projection.phase() else {
            return Ok(DeploymentStatusV1::NotApplicable);
        };
        let runtime = self
            .deployments
            .load_exact_deployment_status(AuthorizedDeploymentStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                exact_deployment,
            ))
            .await?;
        validate_runtime_projection(exact_deployment, &runtime)?;
        deployment_status(exact_deployment, runtime)
    }

    async fn resolve_product_status(
        &self,
        actor: &AuthenticatedActorV1,
        authorized: &AuthorizedInstallationV1<G::Evidence>,
        projection: &ProductDecisionProjectionV1,
    ) -> Result<ProductStatusV1, ProductApplicationError> {
        if let Some(status) = map_non_applied_status(projection.phase()) {
            return Ok(status);
        }
        let ProductDecisionPhaseV1::Applied { exact_deployment } = projection.phase() else {
            return Err(ProductApplicationError::InvalidProjection);
        };
        let runtime = self
            .deployments
            .load_exact_deployment_status(AuthorizedDeploymentStatusV1::new(
                actor,
                authorized.scope(),
                authorized.evidence(),
                exact_deployment,
            ))
            .await?;
        validate_runtime_projection(exact_deployment, &runtime)?;
        Ok(if validate_exact_live(exact_deployment, &runtime) {
            ProductStatusV1::Live
        } else {
            ProductStatusV1::RuntimePending
        })
    }
}

fn validate_preview(
    scope: &crate::AuthorizedInstallationScopeV1,
    promotion: &crate::PromotionSelectorV1,
    preview: &ProductApprovalPreviewV1,
) -> Result<(), ProductApplicationError> {
    let payload = preview.payload();
    if preview.installation_id() != scope.installation_id()
        || preview.guild_id() != scope.guild_id()
        || payload.promotion_id != *promotion.promotion_id()
        || payload.authority.tenant_id != *scope.tenant_id()
        || payload.authority.installation_id != *scope.installation_id()
        || payload.authority.guild_id != scope.guild_id()
    {
        return Err(ProductApplicationError::InvalidProjection);
    }
    if let ProductDecisionPhaseV1::Applied { exact_deployment } = preview.phase() {
        if exact_deployment.installation_id() != scope.installation_id()
            || exact_deployment.promotion_id() != promotion.promotion_id()
        {
            return Err(ProductApplicationError::InvalidProjection);
        }
    }
    let digest = approval_payload_digest_v1(payload)
        .map_err(|_| ProductApplicationError::InvalidProjection)?;
    if digest.to_string() != preview.payload_digest().as_str() {
        return Err(ProductApplicationError::InvalidProjection);
    }
    Ok(())
}

fn deployment_status(
    exact_deployment: &crate::ExactDeploymentSelectorV1,
    status: DeploymentStatusProjectionV1,
) -> Result<DeploymentStatusV1, ProductApplicationError> {
    match status {
        DeploymentStatusProjectionV1::NotRequested => Ok(DeploymentStatusV1::NotRequested),
        DeploymentStatusProjectionV1::Pending => Ok(DeploymentStatusV1::Pending),
        DeploymentStatusProjectionV1::Failed {
            retryable,
            failure_code,
        } => Ok(DeploymentStatusV1::Failed {
            retryable,
            failure_code,
        }),
        DeploymentStatusProjectionV1::ExactLive(live)
            if live.exact_deployment() == exact_deployment =>
        {
            Ok(DeploymentStatusV1::Live {
                attestation_revision: live.attestation_revision(),
            })
        }
        DeploymentStatusProjectionV1::ExactLive(_) => {
            Err(ProductApplicationError::InvalidProjection)
        }
    }
}

fn validate_runtime_projection(
    expected: &crate::ExactDeploymentSelectorV1,
    status: &DeploymentStatusProjectionV1,
) -> Result<(), ProductApplicationError> {
    match status {
        DeploymentStatusProjectionV1::Failed { failure_code, .. }
            if failure_code.is_empty()
                || failure_code.len() > 64
                || !failure_code.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                }) =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        DeploymentStatusProjectionV1::ExactLive(_) if !validate_exact_live(expected, status) => {
            Err(ProductApplicationError::InvalidProjection)
        }
        _ => Ok(()),
    }
}
