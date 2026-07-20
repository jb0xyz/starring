use authoring_application::{
    AuthorizedDeploymentStatusV1, DeploymentStatusObservationPort, DeploymentStatusObservationV1,
    DeploymentStatusPort, DeploymentStatusPortError, DeploymentStatusProjectionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_convergence::{DeploymentId, InstallationId, TenantId};
use automation_runtime_convergence_postgres::{
    project_runtime_deployment_status_v1, RuntimeDeploymentScopeV1,
    RuntimeDeploymentStatusEvidenceV1, RuntimeDeploymentStatusExpectationV1,
};
use sqlx::postgres::PgPool;

mod config;
mod contract;
mod operational;
mod projection;
mod query;
mod readiness;
mod row;

pub use config::{PostgresProductDeploymentStatusesConfig, ProductDeploymentStatusConfigError};
pub use operational::{
    PostgresProductDeploymentOperationalStatusesV2,
    ProductDeploymentOperationalStatusReadinessErrorV2,
};
pub use readiness::ProductDeploymentStatusReadinessErrorV1;

use config::PostgresProductDeploymentStatusesConfig as StatusConfig;
use projection::{
    map_database_error, map_projector_error, project_status, validate_runtime_projection,
};
use query::load_status_rows;
use row::{indeterminate, select_exact_evidence, validate_request_scope};

#[derive(Clone)]
pub struct PostgresProductDeploymentStatuses {
    pub(super) pool: PgPool,
    pub(super) config: StatusConfig,
}

impl PostgresProductDeploymentStatuses {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: StatusConfig::default(),
        }
    }

    pub fn with_config(pool: PgPool, config: StatusConfig) -> Self {
        Self { pool, config }
    }
}

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDeploymentStatuses {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        Ok(self.load_observation(request).await?.into_projection())
    }
}

impl DeploymentStatusObservationPort<FreshDiscordAuthorityEvidenceV1>
    for PostgresProductDeploymentStatuses
{
    async fn load_exact_deployment_observation(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError> {
        self.load_observation(request).await
    }
}

impl PostgresProductDeploymentStatuses {
    async fn load_observation(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusObservationV1, DeploymentStatusPortError> {
        validate_request_scope(&request)?;
        let rows = load_status_rows(&self.pool, self.config, &request)
            .await
            .map_err(map_database_error)?;
        let evidence = select_exact_evidence(rows, &request)?;
        let database_now = evidence.database_now;
        let expectation = runtime_expectation(&request)?;
        let status = project_runtime_deployment_status_v1(
            &expectation,
            database_now,
            RuntimeDeploymentStatusEvidenceV1 {
                deployment_projection: evidence.deployment_projection,
                activation_projection: evidence.activation_projection,
                promotion_projection: evidence.promotion_projection,
                tenant_lifecycle_state: evidence.tenant_lifecycle_state,
                installation_projection: evidence.installation_projection,
                historical_authority_projection: evidence.historical_authority_projection,
                current_authority_projection: evidence.current_authority_projection,
                active_target_version: evidence.active_target_version,
                artifact_projection: evidence.artifact_projection,
                attestation_projection: evidence.attestation_projection,
                serving_projection: evidence.serving_projection,
            },
        )
        .map_err(map_projector_error)?;
        if status.observed_at != database_now {
            return Err(indeterminate());
        }
        validate_runtime_projection(&request, &status)?;
        let projection = project_status(request.exact_deployment(), &status)?;
        let (last_heartbeat_at, lease_expires_at) = status
            .live
            .as_ref()
            .map(|live| {
                (
                    Some(live.last_heartbeat_at.into()),
                    Some(live.expires_at.into()),
                )
            })
            .unwrap_or((None, None));
        DeploymentStatusObservationV1::from_server_projection(
            projection,
            status.observed_at.into(),
            last_heartbeat_at,
            lease_expires_at,
        )
        .map_err(|_| indeterminate())
    }
}

fn runtime_expectation(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<RuntimeDeploymentStatusExpectationV1, DeploymentStatusPortError> {
    let scope = request.scope();
    let exact = request.exact_deployment();
    let evidence = request.evidence();
    let runtime_scope = RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(scope.tenant_id().as_str()).map_err(|_| indeterminate())?,
        installation_id: InstallationId::parse(scope.installation_id().as_str())
            .map_err(|_| indeterminate())?,
        deployment_id: DeploymentId::parse(exact.deployment_reference())
            .map_err(|_| indeterminate())?,
    };
    RuntimeDeploymentStatusExpectationV1::new(
        runtime_scope,
        exact.promotion_id().as_str(),
        exact.target_digest(),
        scope.guild_id().to_string(),
        evidence.application_id().get().to_string(),
        evidence.installation_authority_revision().get(),
        evidence.installation_authority_digest(),
    )
    .map_err(map_projector_error)
}
