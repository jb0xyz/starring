use super::projection_validation::{
    deployment_status, validate_runtime_observation, validate_runtime_operational_observation,
    validate_runtime_projection,
};
use super::ProductControlApplication;
use crate::status::{map_non_applied_status, validate_decision_projection, validate_exact_live};
use crate::{
    AuthenticatedActorV1, AuthenticationPort, AuthorizedDeploymentStatusV1,
    AuthorizedInstallationV1, AuthorizedProductStatusV1, CapabilityV1,
    DeploymentOperationalStatusPortV2, DeploymentStatusObservationPort, DeploymentStatusPort,
    DeploymentStatusV1, FreshGuildAuthorityPort, InstallationSelectorV1, ProductApplicationError,
    ProductDecisionObservationPort, ProductDecisionObservationV1, ProductDecisionPhaseV1,
    ProductDecisionProjectionV1, ProductDecisionQueryPort, ProductDeploymentOperationalStatusV2,
    ProductDeploymentStatusObservationV1, ProductStatusObservationV1, ProductStatusQueryV1,
    ProductStatusV1, RuntimeDeploymentQueryV1,
};

impl<A, G, D, R> ProductControlApplication<'_, A, G, D, R>
where
    A: AuthenticationPort,
    G: FreshGuildAuthorityPort,
{
    pub async fn check_apply_authority(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
    ) -> Result<(), ProductApplicationError> {
        self.authenticate_and_authorize(credential, installation, CapabilityV1::Apply)
            .await
            .map(drop)
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

    pub async fn get_deployment_operational_status_v2(
        &self,
        credential: &A::Credential,
        installation: &InstallationSelectorV1,
        query: RuntimeDeploymentQueryV1,
    ) -> Result<ProductDeploymentOperationalStatusV2, ProductApplicationError>
    where
        D: ProductDecisionObservationPort<G::Evidence>,
        R: DeploymentOperationalStatusPortV2<G::Evidence>,
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
                ProductDeploymentOperationalStatusV2::from_verified_application(
                    DeploymentStatusV1::NotApplicable,
                    decision.projection().clone(),
                    decision.observed_at(),
                    None,
                ),
            );
        };
        let runtime = self
            .deployments
            .load_exact_deployment_operational_status_v2(AuthorizedDeploymentStatusV1::new(
                &actor,
                authorized.scope(),
                authorized.evidence(),
                exact_deployment,
            ))
            .await?;
        validate_runtime_operational_observation(
            exact_deployment,
            decision.observed_at(),
            &runtime,
        )?;
        let status = deployment_status(exact_deployment, runtime.base().projection().clone())?;
        Ok(
            ProductDeploymentOperationalStatusV2::from_verified_application(
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

    pub(super) async fn resolve_product_status(
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
