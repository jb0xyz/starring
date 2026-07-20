use std::fmt::{Debug, Formatter};

use async_trait::async_trait;
use authoring_application::{
    AuthoringApplication, AuthorizedDeploymentStatusV1, DeploymentOperationalObservationV2,
    DeploymentOperationalStatusPortV2, DeploymentStatusObservationPort,
    DeploymentStatusObservationV1, DeploymentStatusPort, DeploymentStatusPortError,
    DeploymentStatusProjectionV1, ProductControlApplication, ProductStatusQueryV1,
    PromotionSelectorV1,
};
use authoring_application_discord::{
    DiscordAuthorityConfigV1, DiscordGuildAuthorityAdapter, DiscordOAuthClient,
    DiscordOAuthClientSecretV1, FreshDiscordAuthorityEvidenceV1,
    TwilightDiscordGuildAuthorityClient,
};
use authoring_application_postgres::{
    OperatingSystemSecretGenerator, PostgresAuthentication, PostgresAuthorizedPromotionSnapshots,
    PostgresInstallationAuthoritySource, PostgresProductApiReadiness, PostgresProductControl,
    PostgresProductDeploymentOperationalStatusesV2, PostgresProductDeploymentStatuses,
    PostgresProductIdentityStore, PostgresProductPromotions, ProductApiReadinessErrorV1,
    XChaCha20Poly1305SnapshotEnvelopeCipherV1,
};
use product_control_http::{
    ApplyCommand, ApplyView, ApprovalPreviewView, CsrfSecret, CurrentPrincipal, DecisionCommand,
    DecisionView, DeploymentOperationalViewV2, DeploymentView, FacadeError, FacadeErrorCode,
    OAuthCallbackCommand, OAuthCallbackResult, OAuthStartCommand, OAuthStartResult,
    ProductControlFacade, ProductControlOperationalFacadeV2, PromoteCommand, PromotionView,
    RejectCommand, SessionCredential,
};

use crate::{
    map_apply_command, map_approve_command, map_authoring_application_error,
    map_discord_authorization_code, map_discord_oauth_error, map_discord_oauth_state,
    map_oauth_flow_error, map_product_application_error, map_product_identity_error,
    map_product_target, map_promote_command, map_reject_command, project_apply,
    project_approval_preview, project_current_principal, project_decision_mutation,
    project_deployment, project_deployment_operational_v2, project_oauth_callback,
    project_oauth_start, project_product_status, project_promotion,
};

type ProductionIdentityStore = PostgresProductIdentityStore<OperatingSystemSecretGenerator>;
type ProductionSnapshots =
    PostgresAuthorizedPromotionSnapshots<XChaCha20Poly1305SnapshotEnvelopeCipherV1>;
type ProductionAuthority = DiscordGuildAuthorityAdapter<
    PostgresInstallationAuthoritySource,
    TwilightDiscordGuildAuthorityClient,
>;

pub struct ProductionIdentityDependenciesV1 {
    identity: ProductionIdentityStore,
    oauth: DiscordOAuthClient,
    oauth_client_secret: DiscordOAuthClientSecretV1,
    default_return_path: String,
}

impl ProductionIdentityDependenciesV1 {
    pub fn new(
        identity: ProductionIdentityStore,
        oauth: DiscordOAuthClient,
        oauth_client_secret: DiscordOAuthClientSecretV1,
        default_return_path: String,
    ) -> Self {
        Self {
            identity,
            oauth,
            oauth_client_secret,
            default_return_path,
        }
    }
}

impl Debug for ProductionIdentityDependenciesV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionIdentityDependenciesV1(<redacted>)")
    }
}

pub struct ProductionAuthorityDependenciesV1 {
    installation_authority: PostgresInstallationAuthoritySource,
    discord_authority: TwilightDiscordGuildAuthorityClient,
    config: DiscordAuthorityConfigV1,
}

impl ProductionAuthorityDependenciesV1 {
    pub fn new(
        installation_authority: PostgresInstallationAuthoritySource,
        discord_authority: TwilightDiscordGuildAuthorityClient,
        config: DiscordAuthorityConfigV1,
    ) -> Self {
        Self {
            installation_authority,
            discord_authority,
            config,
        }
    }
}

impl Debug for ProductionAuthorityDependenciesV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionAuthorityDependenciesV1(<redacted>)")
    }
}

pub struct ProductionPersistenceDependenciesV1 {
    snapshots: ProductionSnapshots,
    promotions: PostgresProductPromotions,
    control: PostgresProductControl,
    deployment_statuses: PostgresProductDeploymentStatuses,
    operational_deployment_statuses: PostgresProductDeploymentOperationalStatusesV2,
}

impl ProductionPersistenceDependenciesV1 {
    pub fn new(
        snapshots: ProductionSnapshots,
        promotions: PostgresProductPromotions,
        control: PostgresProductControl,
        deployment_statuses: PostgresProductDeploymentStatuses,
        operational_deployment_statuses: PostgresProductDeploymentOperationalStatusesV2,
    ) -> Self {
        Self {
            snapshots,
            promotions,
            control,
            deployment_statuses,
            operational_deployment_statuses,
        }
    }
}

impl Debug for ProductionPersistenceDependenciesV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionPersistenceDependenciesV1(<redacted>)")
    }
}

pub struct ProductionProductControlFacadeV1 {
    identity: ProductionIdentityStore,
    authentication: PostgresAuthentication,
    oauth: DiscordOAuthClient,
    oauth_client_secret: DiscordOAuthClientSecretV1,
    default_return_path: String,
    installation_authority: PostgresInstallationAuthoritySource,
    authority: ProductionAuthority,
    snapshots: ProductionSnapshots,
    promotions: PostgresProductPromotions,
    control: PostgresProductControl,
    deployments: ProductionDeploymentStatusesV1,
}

impl ProductionProductControlFacadeV1 {
    pub fn new(
        identity: ProductionIdentityDependenciesV1,
        authority: ProductionAuthorityDependenciesV1,
        persistence: ProductionPersistenceDependenciesV1,
    ) -> Self {
        let authentication = identity.identity.authentication();
        let installation_authority = authority.installation_authority;
        let guild_authority = DiscordGuildAuthorityAdapter::new(
            installation_authority.clone(),
            authority.discord_authority,
            authority.config,
        );
        Self {
            identity: identity.identity,
            authentication,
            oauth: identity.oauth,
            oauth_client_secret: identity.oauth_client_secret,
            default_return_path: identity.default_return_path,
            installation_authority,
            authority: guild_authority,
            snapshots: persistence.snapshots,
            promotions: persistence.promotions,
            control: persistence.control,
            deployments: ProductionDeploymentStatusesV1 {
                status: persistence.deployment_statuses,
                operational: persistence.operational_deployment_statuses,
            },
        }
    }

    fn authoring_application(
        &self,
    ) -> AuthoringApplication<
        '_,
        PostgresAuthentication,
        ProductionAuthority,
        ProductionSnapshots,
        PostgresProductPromotions,
    > {
        AuthoringApplication::new(
            &self.authentication,
            &self.authority,
            &self.snapshots,
            &self.promotions,
        )
    }

    fn control_application(
        &self,
    ) -> ProductControlApplication<
        '_,
        PostgresAuthentication,
        ProductionAuthority,
        PostgresProductControl,
        ProductionDeploymentStatusesV1,
    > {
        ProductControlApplication::new(
            &self.authentication,
            &self.authority,
            &self.control,
            &self.deployments,
        )
    }
}

impl Debug for ProductionProductControlFacadeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductionProductControlFacadeV1(<redacted>)")
    }
}

struct ProductionDeploymentStatusesV1 {
    status: PostgresProductDeploymentStatuses,
    operational: PostgresProductDeploymentOperationalStatusesV2,
}

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for ProductionDeploymentStatusesV1 {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        self.status.load_exact_deployment_status(request).await
    }
}

impl DeploymentStatusObservationPort<FreshDiscordAuthorityEvidenceV1>
    for ProductionDeploymentStatusesV1
{
    async fn load_exact_deployment_observation(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError> {
        self.status.load_exact_deployment_observation(request).await
    }
}

impl DeploymentOperationalStatusPortV2<FreshDiscordAuthorityEvidenceV1>
    for ProductionDeploymentStatusesV1
{
    async fn load_exact_deployment_operational_status_v2(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentOperationalObservationV2, DeploymentStatusPortError> {
        self.operational
            .load_exact_deployment_operational_status_v2(request)
            .await
    }
}

#[async_trait]
impl ProductControlFacade for ProductionProductControlFacadeV1 {
    async fn oauth_start(
        &self,
        command: OAuthStartCommand,
    ) -> Result<OAuthStartResult, FacadeError> {
        let return_path = command
            .return_to
            .as_deref()
            .unwrap_or(&self.default_return_path);
        let issue = self
            .identity
            .create_oauth_flow(return_path)
            .await
            .map_err(map_oauth_flow_error)?;
        if issue.redirect_uri() != self.oauth.redirect_uri() {
            return Err(internal());
        }
        project_oauth_start(&issue, self.oauth.client_id())
    }

    async fn oauth_callback(
        &self,
        command: OAuthCallbackCommand,
    ) -> Result<OAuthCallbackResult, FacadeError> {
        let state = map_discord_oauth_state(&command.state)?;
        let browser_nonce = map_discord_oauth_state(&command.browser_nonce)?;
        let consumed = self
            .identity
            .consume_oauth_flow(state.expose_secret(), browser_nonce.expose_secret())
            .await
            .map_err(map_oauth_flow_error)?;
        if consumed.redirect_uri() != self.oauth.redirect_uri() {
            return Err(internal());
        }
        let authorization_code = map_discord_authorization_code(&command.code)?;
        let verified_identity = self
            .oauth
            .exchange_identify(&authorization_code, &self.oauth_client_secret)
            .await
            .map_err(map_discord_oauth_error)?;
        let session = self
            .identity
            .issue_product_session(consumed, verified_identity)
            .await
            .map_err(map_product_identity_error)?;
        project_oauth_callback(&session)
    }

    async fn current_principal(
        &self,
        credential: &SessionCredential,
    ) -> Result<CurrentPrincipal, FacadeError> {
        let principal = self
            .identity
            .current_principal(credential.expose_secret())
            .await
            .map_err(map_product_identity_error)?;
        Ok(project_current_principal(&principal))
    }

    async fn revoke_session(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
    ) -> Result<(), FacadeError> {
        self.identity
            .logout(credential.expose_secret(), csrf.expose_secret())
            .await
            .map(drop)
            .map_err(map_product_identity_error)
    }

    async fn promote(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: PromoteCommand,
    ) -> Result<PromotionView, FacadeError> {
        let (request_id, installation, promotion_command) =
            map_promote_command(command)?.into_parts();
        let promotion = self
            .authoring_application()
            .promote_owned_session_observation(
                credential.expose_secret(),
                csrf.expose_secret(),
                &request_id,
                &installation,
                promotion_command,
            )
            .await
            .map_err(map_authoring_application_error)?;
        let current = self
            .control_application()
            .get_product_status_observation(
                credential.expose_secret(),
                &installation,
                ProductStatusQueryV1 {
                    promotion: PromotionSelectorV1::new(promotion.promotion_id().clone()),
                },
            )
            .await
            .map_err(map_product_application_error)?;
        project_promotion(&promotion, &current)
    }

    async fn status(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<DecisionView, FacadeError> {
        let target = map_product_target(installation_id, promotion_id)?;
        let observation = self
            .control_application()
            .get_product_status_observation(
                credential.expose_secret(),
                target.installation(),
                target.status_query(),
            )
            .await
            .map_err(map_product_application_error)?;
        project_product_status(&observation)
    }

    async fn approval_preview(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<ApprovalPreviewView, FacadeError> {
        let target = map_product_target(installation_id, promotion_id)?;
        let observation = self
            .control_application()
            .get_approval_preview_observation(
                credential.expose_secret(),
                target.installation(),
                target.status_query(),
            )
            .await
            .map_err(map_product_application_error)?;
        project_approval_preview(&observation)
    }

    async fn approve(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: DecisionCommand,
    ) -> Result<DecisionView, FacadeError> {
        let (request_id, installation, command) = map_approve_command(command)?.into_parts();
        let receipt = self
            .control_application()
            .approve(
                credential.expose_secret(),
                csrf.expose_secret(),
                &request_id,
                &installation,
                command,
            )
            .await
            .map_err(map_product_application_error)?;
        Ok(project_decision_mutation(&receipt))
    }

    async fn reject(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: RejectCommand,
    ) -> Result<DecisionView, FacadeError> {
        let (request_id, installation, command) = map_reject_command(command)?.into_parts();
        let receipt = self
            .control_application()
            .reject(
                credential.expose_secret(),
                csrf.expose_secret(),
                &request_id,
                &installation,
                command,
            )
            .await
            .map_err(map_product_application_error)?;
        Ok(project_decision_mutation(&receipt))
    }

    async fn apply(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: ApplyCommand,
    ) -> Result<ApplyView, FacadeError> {
        let (request_id, installation, command) = map_apply_command(command)?.into_parts();
        let result = self
            .control_application()
            .apply(
                credential.expose_secret(),
                csrf.expose_secret(),
                &request_id,
                &installation,
                command,
            )
            .await
            .map_err(map_product_application_error)?;
        Ok(project_apply(&result))
    }

    async fn deployment(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<DeploymentView, FacadeError> {
        let target = map_product_target(installation_id, promotion_id)?;
        let observation = self
            .control_application()
            .get_deployment_status_observation(
                credential.expose_secret(),
                target.installation(),
                target.runtime_query(),
            )
            .await
            .map_err(map_product_application_error)?;
        project_deployment(&observation)
    }

    async fn readiness(&self) -> Result<(), FacadeError> {
        PostgresProductApiReadiness::new(
            &self.identity,
            &self.installation_authority,
            &self.snapshots,
            &self.promotions,
            &self.control,
            &self.deployments.status,
            &self.deployments.operational,
        )
        .verify_readiness()
        .await
        .map_err(map_readiness_error)
    }
}

#[async_trait]
impl ProductControlOperationalFacadeV2 for ProductionProductControlFacadeV1 {
    async fn deployment_operational_v2(
        &self,
        credential: &SessionCredential,
        installation_id: &str,
        promotion_id: &str,
    ) -> Result<DeploymentOperationalViewV2, FacadeError> {
        let target = map_product_target(installation_id, promotion_id)?;
        let observation = self
            .control_application()
            .get_deployment_operational_status_v2(
                credential.expose_secret(),
                target.installation(),
                target.runtime_query(),
            )
            .await
            .map_err(map_product_application_error)?;
        project_deployment_operational_v2(&observation)
    }
}

fn map_readiness_error(error: ProductApiReadinessErrorV1) -> FacadeError {
    match error {
        ProductApiReadinessErrorV1::Identity(_)
        | ProductApiReadinessErrorV1::InstallationAuthority(_)
        | ProductApiReadinessErrorV1::AuthorizedSnapshot(_)
        | ProductApiReadinessErrorV1::Promotion(_)
        | ProductApiReadinessErrorV1::Decision(_)
        | ProductApiReadinessErrorV1::DeploymentStatus(_)
        | ProductApiReadinessErrorV1::OperationalDeploymentStatus(_)
        | ProductApiReadinessErrorV1::TopologyMismatch => {
            FacadeError::new(FacadeErrorCode::DependencyUnavailable)
        }
    }
}

fn internal() -> FacadeError {
    FacadeError::new(FacadeErrorCode::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_operational_facade<T: ProductControlOperationalFacadeV2>() {}

    #[test]
    fn production_facade_closes_the_complete_http_contract() {
        assert_operational_facade::<ProductionProductControlFacadeV1>();
    }

    #[test]
    fn topology_drift_keeps_readiness_closed_and_retryable() {
        let error = map_readiness_error(ProductApiReadinessErrorV1::TopologyMismatch);
        assert_eq!(error.error_code(), FacadeErrorCode::DependencyUnavailable);
        assert!(error.retryable());
    }
}
