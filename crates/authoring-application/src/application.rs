use authoring_promotion::{approval_payload_digest_v1, plan_start_promotion_ref_v1};

use crate::authority::validate_authorized_scope;
use crate::promotion::{
    build_start_promotion, product_promotion_observation, validate_authorized_replay,
    validate_authorized_submission, ExpectedPromotionSubmissionV1,
};
use crate::status::{map_non_applied_status, validate_decision_projection, validate_exact_live};
use crate::{
    ApplyProductPromotionV1, AuthenticatedActorV1, AuthenticationPort, AuthorizedApplyProductV1,
    AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1, AuthorizedDeploymentStatusV1,
    AuthorizedInstallationV1, AuthorizedProductStatusV1, AuthorizedPromotionAccessV1,
    AuthorizedPromotionSnapshotPort, AuthorizedPromotionSubmissionPort,
    AuthorizedPromotionSubmissionV1, AuthorizedRejectProductV1, CapabilityV1,
    DeploymentStatusObservationPort, DeploymentStatusObservationV1, DeploymentStatusPort,
    DeploymentStatusProjectionV1, DeploymentStatusV1, FreshGuildAuthorityPort,
    InstallationSelectorV1, MutationAuthenticationPort, ProductApplicationError, ProductApplyPort,
    ProductApplyResultV1, ProductApprovalPort, ProductApprovalPreviewObservationV1,
    ProductApprovalPreviewV1, ProductControlPortError, ProductDecisionObservationPort,
    ProductDecisionObservationV1, ProductDecisionPhaseV1, ProductDecisionProjectionV1,
    ProductDecisionQueryPort, ProductDeploymentStatusObservationV1, ProductMutationReceiptV1,
    ProductPromotionObservationV1, ProductRejectionPort, ProductRequestIdV1,
    ProductStatusObservationV1, ProductStatusQueryV1, ProductStatusV1, PromoteOwnedSessionV1,
    PromotionSubmissionV1, RejectProductPromotionV1, RuntimeDeploymentQueryV1,
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
    P: AuthorizedPromotionSubmissionPort<G::Evidence>,
{
    pub async fn promote_owned_session(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
    ) -> Result<PromotionSubmissionV1, AuthoringApplicationError> {
        Ok(self
            .promote_owned_session_inner(credential, csrf, request_id, installation, command, false)
            .await?
            .0)
    }

    pub async fn promote_owned_session_observation(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
    ) -> Result<ProductPromotionObservationV1, AuthoringApplicationError> {
        let (_, observation) = self
            .promote_owned_session_inner(credential, csrf, request_id, installation, command, true)
            .await?;
        observation.ok_or({
            AuthoringApplicationError::AuthorizedPromotion(
                crate::AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt,
            )
        })
    }

    async fn promote_owned_session_inner(
        &self,
        credential: &A::Credential,
        csrf: &A::CsrfProof,
        request_id: &ProductRequestIdV1,
        installation: &InstallationSelectorV1,
        command: PromoteOwnedSessionV1,
        include_observation: bool,
    ) -> Result<
        (PromotionSubmissionV1, Option<ProductPromotionObservationV1>),
        AuthoringApplicationError,
    > {
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
        let access = AuthorizedPromotionAccessV1::new(
            request_id,
            &actor,
            authorized.scope(),
            authorized.evidence(),
            command,
        );
        if let Some(submission) = self
            .promotions
            .find_or_resume_authorized_promotion(&access)
            .await?
        {
            let record = validate_authorized_replay(&access, &submission)?;
            let observation = include_observation
                .then(|| product_promotion_observation(record, submission.disposition))
                .transpose()?;
            return Ok((submission, observation));
        }
        let snapshot = self
            .snapshots
            .load_atomic_authorized_snapshot(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                access.session_id(),
                access.expected_generation(),
            )
            .await?;
        let input = build_start_promotion(&actor, authorized.scope(), &access, snapshot)?;
        let plan = plan_start_promotion_ref_v1(&input)?;
        let expected = ExpectedPromotionSubmissionV1::from_plan(&plan);
        let submission = self
            .promotions
            .submit_authorized_promotion(AuthorizedPromotionSubmissionV1::new(access, input, plan))
            .await?;
        let record = validate_authorized_submission(&expected, &submission)?;
        let observation = include_observation
            .then(|| product_promotion_observation(record, submission.disposition))
            .transpose()?;
        Ok((submission, observation))
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

    pub async fn get_product_status(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: ProductStatusQueryV1,
    ) -> Result<ProductStatusV1, ProductApplicationError>
    where
        D: ProductDecisionQueryPort<G::Evidence>,
        R: DeploymentStatusPort<G::Evidence>,
    {
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

    pub async fn get_product_status_observation(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: ProductStatusQueryV1,
    ) -> Result<ProductStatusObservationV1, ProductApplicationError>
    where
        D: ProductDecisionObservationPort<G::Evidence>,
        R: DeploymentStatusObservationPort<G::Evidence>,
    {
        let (actor, authorized) = self
            .authenticate_and_authorize(credential, installation, CapabilityV1::Read)
            .await?;
        let observation = self
            .decisions
            .load_product_status_observation(AuthorizedProductStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &query.promotion,
            ))
            .await?;
        validate_decision_projection(
            authorized.scope(),
            &query.promotion,
            observation.projection(),
        )?;
        self.resolve_product_status_observation(&actor, &authorized, observation)
            .await
    }

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

    pub async fn get_deployment_status(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: RuntimeDeploymentQueryV1,
    ) -> Result<DeploymentStatusV1, ProductApplicationError>
    where
        D: ProductDecisionQueryPort<G::Evidence>,
        R: DeploymentStatusPort<G::Evidence>,
    {
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

    pub async fn get_deployment_status_observation(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: RuntimeDeploymentQueryV1,
    ) -> Result<ProductDeploymentStatusObservationV1, ProductApplicationError>
    where
        D: ProductDecisionObservationPort<G::Evidence>,
        R: DeploymentStatusObservationPort<G::Evidence>,
    {
        let (actor, authorized) = self
            .authenticate_and_authorize(credential, installation, CapabilityV1::Read)
            .await?;
        let decision = self
            .decisions
            .load_product_status_observation(AuthorizedProductStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                &query.promotion,
            ))
            .await?;
        validate_decision_projection(authorized.scope(), &query.promotion, decision.projection())?;
        let ProductDecisionPhaseV1::Applied { exact_deployment } = decision.projection().phase()
        else {
            return Ok(
                ProductDeploymentStatusObservationV1::from_verified_application(
                    DeploymentStatusV1::NotApplicable,
                    decision.projection().clone(),
                    decision.observed_at(),
                    None,
                ),
            );
        };
        let runtime = self
            .deployments
            .load_exact_deployment_observation(AuthorizedDeploymentStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                exact_deployment,
            ))
            .await?;
        validate_runtime_observation(exact_deployment, decision.observed_at(), &runtime)?;
        let status = deployment_status(exact_deployment, runtime.projection().clone())?;
        Ok(
            ProductDeploymentStatusObservationV1::from_verified_application(
                status,
                decision.projection().clone(),
                decision.observed_at(),
                Some(runtime),
            ),
        )
    }

    async fn resolve_product_status_observation(
        &self,
        actor: &AuthenticatedActorV1,
        authorized: &AuthorizedInstallationV1<G::Evidence>,
        decision: ProductDecisionObservationV1,
    ) -> Result<ProductStatusObservationV1, ProductApplicationError>
    where
        R: DeploymentStatusObservationPort<G::Evidence>,
    {
        if let Some(status) = map_non_applied_status(decision.projection().phase()) {
            return Ok(ProductStatusObservationV1::from_verified_application(
                status,
                decision.projection().clone(),
                decision.observed_at(),
                None,
            ));
        }
        let ProductDecisionPhaseV1::Applied { exact_deployment } = decision.projection().phase()
        else {
            return Err(ProductApplicationError::InvalidProjection);
        };
        let runtime = self
            .deployments
            .load_exact_deployment_observation(AuthorizedDeploymentStatusV1::new(
                actor,
                authorized.scope(),
                authorized.evidence(),
                exact_deployment,
            ))
            .await?;
        validate_runtime_observation(exact_deployment, decision.observed_at(), &runtime)?;
        let status = if validate_exact_live(exact_deployment, runtime.projection()) {
            ProductStatusV1::Live
        } else {
            ProductStatusV1::RuntimePending
        };
        Ok(ProductStatusObservationV1::from_verified_application(
            status,
            decision.projection().clone(),
            decision.observed_at(),
            Some(runtime),
        ))
    }

    async fn resolve_product_status(
        &self,
        actor: &AuthenticatedActorV1,
        authorized: &AuthorizedInstallationV1<G::Evidence>,
        projection: &ProductDecisionProjectionV1,
    ) -> Result<ProductStatusV1, ProductApplicationError>
    where
        R: DeploymentStatusPort<G::Evidence>,
    {
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

fn validate_approval_phase(phase: &ProductDecisionPhaseV1) -> Result<(), ProductApplicationError> {
    if matches!(
        phase,
        ProductDecisionPhaseV1::PendingApproval | ProductDecisionPhaseV1::Approved
    ) {
        Ok(())
    } else {
        Err(ProductApplicationError::InvalidProjection)
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

fn validate_preview_observation(
    scope: &crate::AuthorizedInstallationScopeV1,
    promotion: &crate::PromotionSelectorV1,
    observation: &ProductApprovalPreviewObservationV1,
) -> Result<(), ProductApplicationError> {
    validate_preview(scope, promotion, observation.preview())?;
    match observation.preview().phase() {
        ProductDecisionPhaseV1::PendingApproval | ProductDecisionPhaseV1::Approved
            if observation.observed_at() >= observation.activation_expires_at() =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        ProductDecisionPhaseV1::Expired
            if observation.observed_at() < observation.activation_expires_at() =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        _ => Ok(()),
    }
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
            if crate::DeploymentFailureCodeV1::parse(failure_code).is_err() =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        DeploymentStatusProjectionV1::ExactLive(_) if !validate_exact_live(expected, status) => {
            Err(ProductApplicationError::InvalidProjection)
        }
        _ => Ok(()),
    }
}

fn validate_runtime_observation(
    expected: &crate::ExactDeploymentSelectorV1,
    decision_observed_at: std::time::SystemTime,
    observation: &DeploymentStatusObservationV1,
) -> Result<(), ProductApplicationError> {
    if observation.observed_at() < decision_observed_at {
        return Err(ProductApplicationError::InvalidProjection);
    }
    validate_runtime_projection(expected, observation.projection())
}
