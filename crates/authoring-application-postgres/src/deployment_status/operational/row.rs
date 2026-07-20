use authoring_application::{AuthorizedDeploymentStatusV1, DeploymentStatusPortError};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use super::super::row::ProductDeploymentStatusRow;
use super::super::row::{indeterminate, select_exact_evidence, ProductDeploymentStatusEvidenceV1};

#[derive(sqlx::FromRow)]
pub(super) struct ProductDeploymentOperationalStatusRow {
    #[sqlx(flatten)]
    base: ProductDeploymentStatusRow,
    deployment_convergence_attempt_no: Option<i64>,
    deployment_last_failure_attempt_no: Option<i64>,
    attestation_convergence_attempt_no: Option<i64>,
}

pub(super) struct ProductDeploymentOperationalStatusEvidenceV2 {
    pub(super) base: ProductDeploymentStatusEvidenceV1,
    pub(super) deployment_convergence_attempt_no: i64,
    pub(super) deployment_last_failure_attempt_no: Option<i64>,
    pub(super) attestation_convergence_attempt_no: Option<i64>,
}

pub(super) fn select_operational_evidence(
    rows: Vec<ProductDeploymentOperationalStatusRow>,
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<ProductDeploymentOperationalStatusEvidenceV2, DeploymentStatusPortError> {
    let [row]: [ProductDeploymentOperationalStatusRow; 1] =
        rows.try_into()
            .map_err(|rows: Vec<ProductDeploymentOperationalStatusRow>| {
                if rows.is_empty() {
                    DeploymentStatusPortError::NotFound
                } else {
                    indeterminate()
                }
            })?;
    let scalar_evidence_empty = row.deployment_convergence_attempt_no.is_none()
        && row.deployment_last_failure_attempt_no.is_none()
        && row.attestation_convergence_attempt_no.is_none();
    if row.base.request_outcome() == "request_mismatch"
        && (!row.base.sensitive_evidence_is_empty() || !scalar_evidence_empty)
    {
        return Err(indeterminate());
    }
    let deployment_convergence_attempt_no = row
        .deployment_convergence_attempt_no
        .ok_or_else(indeterminate)?;
    let base = select_exact_evidence(vec![row.base], request)?;
    Ok(ProductDeploymentOperationalStatusEvidenceV2 {
        base,
        deployment_convergence_attempt_no,
        deployment_last_failure_attempt_no: row.deployment_last_failure_attempt_no,
        attestation_convergence_attempt_no: row.attestation_convergence_attempt_no,
    })
}
