use authoring_application::{
    AuthorizedDeploymentStatusV1, DeploymentOperationalObservationV2,
    DeploymentOperationalStatusPortV2, DeploymentStatusPortError,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_convergence_postgres::{
    project_runtime_deployment_status_v2, RuntimeDeploymentStatusEvidenceV1,
    RuntimeDeploymentStatusEvidenceV2,
};
use sqlx::postgres::PgPool;

mod contract;
mod projection;
mod query;
mod readiness;
mod row;

pub use readiness::ProductDeploymentOperationalStatusReadinessErrorV2;

use super::config::PostgresProductDeploymentStatusesConfig;
use super::projection::{map_database_error, map_projector_error};
use super::row::{indeterminate, validate_request_scope};
use projection::project_operational_status;
use query::load_status_rows;
use row::select_operational_evidence;

#[derive(Clone)]
pub struct PostgresProductDeploymentOperationalStatusesV2 {
    pub(super) pool: PgPool,
    pub(super) config: PostgresProductDeploymentStatusesConfig,
}

impl PostgresProductDeploymentOperationalStatusesV2 {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: PostgresProductDeploymentStatusesConfig::default(),
        }
    }

    pub fn with_config(pool: PgPool, config: PostgresProductDeploymentStatusesConfig) -> Self {
        Self { pool, config }
    }
}

impl DeploymentOperationalStatusPortV2<FreshDiscordAuthorityEvidenceV1>
    for PostgresProductDeploymentOperationalStatusesV2
{
    async fn load_exact_deployment_operational_status_v2(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentOperationalObservationV2, DeploymentStatusPortError> {
        validate_request_scope(&request)?;
        let rows = load_status_rows(&self.pool, self.config, &request)
            .await
            .map_err(map_database_error)?;
        let evidence = select_operational_evidence(rows, &request)?;
        let database_now = evidence.base.database_now;
        let expectation = super::runtime_expectation(&request)?;
        let status = project_runtime_deployment_status_v2(
            &expectation,
            database_now,
            RuntimeDeploymentStatusEvidenceV2 {
                evidence: RuntimeDeploymentStatusEvidenceV1 {
                    deployment_projection: evidence.base.deployment_projection,
                    activation_projection: evidence.base.activation_projection,
                    promotion_projection: evidence.base.promotion_projection,
                    tenant_lifecycle_state: evidence.base.tenant_lifecycle_state,
                    installation_projection: evidence.base.installation_projection,
                    historical_authority_projection: evidence.base.historical_authority_projection,
                    current_authority_projection: evidence.base.current_authority_projection,
                    active_target_version: evidence.base.active_target_version,
                    artifact_projection: evidence.base.artifact_projection,
                    attestation_projection: evidence.base.attestation_projection,
                    serving_projection: evidence.base.serving_projection,
                },
                deployment_convergence_attempt_no: evidence.deployment_convergence_attempt_no,
                deployment_last_failure_attempt_no: evidence.deployment_last_failure_attempt_no,
                attestation_convergence_attempt_no: evidence.attestation_convergence_attempt_no,
            },
        )
        .map_err(map_projector_error)?;
        if status.status.observed_at != database_now {
            return Err(indeterminate());
        }
        project_operational_status(&request, status)
    }
}
