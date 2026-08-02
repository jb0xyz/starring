use authoring_application::{
    AuthorizedDeploymentStatusV1, CapabilityV1, DeploymentStatusPortError,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_convergence_postgres::RuntimeDeploymentAttestationEvidenceV2;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

const MAX_READ_AUTHORITY_LIFETIME: chrono::Duration = chrono::Duration::seconds(30);
const MAX_APPLY_AUTHORITY_LIFETIME: chrono::Duration = chrono::Duration::seconds(5);

#[derive(sqlx::FromRow)]
pub(super) struct ProductDeploymentStatusRow {
    request_outcome: String,
    deployment_projection: Option<Json<Value>>,
    activation_projection: Option<Json<Value>>,
    promotion_projection: Option<Json<Value>>,
    tenant_lifecycle_state: Option<String>,
    installation_projection: Option<Json<Value>>,
    historical_authority_projection: Option<Json<Value>>,
    current_authority_projection: Option<Json<Value>>,
    active_target_version: Option<i64>,
    artifact_projection: Option<Json<Value>>,
    attestation_projection: Option<Json<Value>>,
    serving_projection: Option<Json<Value>>,
    database_now: DateTime<Utc>,
    #[sqlx(flatten)]
    attestation_evidence_v2: ProductDeploymentAttestationEvidenceV2,
}

#[derive(Default, sqlx::FromRow)]
pub(super) struct ProductDeploymentAttestationEvidenceV2 {
    pub(super) attestation_record_format_version: Option<i16>,
    pub(super) attestation_serving_lease_duration_nanos: Option<i64>,
    pub(super) attestation_convergence_attempt_no: Option<i64>,
    pub(super) deployment_last_controller_id: Option<String>,
    pub(super) v2_evidence_state: Option<String>,
    pub(super) v2_operation_id: Option<String>,
    pub(super) v2_intent_fingerprint: Option<String>,
    pub(super) v2_certification_intent_bytes: Option<Vec<u8>>,
    pub(super) v2_request_digest: Option<String>,
    pub(super) v2_request_bytes: Option<Vec<u8>>,
    pub(super) v2_live_attestation_bytes: Option<Vec<u8>>,
    pub(super) v2_must_commit_before: Option<DateTime<Utc>>,
    pub(super) v2_route_admission: Option<Json<Value>>,
    pub(super) v2_certified_snapshot: Option<Json<Value>>,
}

pub(super) struct ProductDeploymentStatusEvidenceV1 {
    pub(super) deployment_projection: Value,
    pub(super) activation_projection: Option<Value>,
    pub(super) promotion_projection: Option<Value>,
    pub(super) tenant_lifecycle_state: Option<String>,
    pub(super) installation_projection: Option<Value>,
    pub(super) historical_authority_projection: Option<Value>,
    pub(super) current_authority_projection: Option<Value>,
    pub(super) active_target_version: Option<i64>,
    pub(super) artifact_projection: Option<Value>,
    pub(super) attestation_projection: Option<Value>,
    pub(super) serving_projection: Option<Value>,
    pub(super) database_now: DateTime<Utc>,
    pub(super) attestation_evidence_v2: ProductDeploymentAttestationEvidenceV2,
}

pub(super) fn validate_request_scope(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), DeploymentStatusPortError> {
    let evidence = request.evidence();
    let scope = request.scope();
    let exact = request.exact_deployment();
    let Some(maximum_lifetime) = status_authority_lifetime(evidence.capability()) else {
        return Err(indeterminate());
    };
    let bounded_window = evidence.observed_at() < evidence.expires_at()
        && evidence
            .observed_at()
            .checked_add_signed(maximum_lifetime)
            .is_some_and(|latest| evidence.expires_at() <= latest);
    if !bounded_window
        || evidence.tenant_id() != scope.tenant_id()
        || evidence.installation_id() != scope.installation_id()
        || evidence.guild_id() != scope.guild_id()
        || evidence.acting_user_id() != scope.acting_user_id()
        || exact.installation_id() != scope.installation_id()
    {
        return Err(indeterminate());
    }
    Ok(())
}

pub(super) fn select_exact_evidence(
    rows: Vec<ProductDeploymentStatusRow>,
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<ProductDeploymentStatusEvidenceV1, DeploymentStatusPortError> {
    let [row]: [ProductDeploymentStatusRow; 1] =
        rows.try_into()
            .map_err(|rows: Vec<ProductDeploymentStatusRow>| {
                if rows.is_empty() {
                    DeploymentStatusPortError::NotFound
                } else {
                    indeterminate()
                }
            })?;
    match row.request_outcome.as_str() {
        "exact" => row.into_exact(request),
        "request_mismatch" if row.sensitive_evidence_is_empty() => Err(indeterminate()),
        _ => Err(indeterminate()),
    }
}

impl ProductDeploymentStatusRow {
    pub(super) fn request_outcome(&self) -> &str {
        &self.request_outcome
    }

    pub(super) fn sensitive_evidence_is_empty(&self) -> bool {
        self.deployment_projection.is_none()
            && self.activation_projection.is_none()
            && self.promotion_projection.is_none()
            && self.tenant_lifecycle_state.is_none()
            && self.installation_projection.is_none()
            && self.historical_authority_projection.is_none()
            && self.current_authority_projection.is_none()
            && self.active_target_version.is_none()
            && self.artifact_projection.is_none()
            && self.attestation_projection.is_none()
            && self.serving_projection.is_none()
            && self.attestation_evidence_v2.is_empty()
    }

    fn into_exact(
        self,
        request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDeploymentStatusEvidenceV1, DeploymentStatusPortError> {
        validate_database_observation(request, self.database_now)?;
        let deployment_projection = self.deployment_projection.ok_or_else(indeterminate)?.0;
        Ok(ProductDeploymentStatusEvidenceV1 {
            deployment_projection,
            activation_projection: self.activation_projection.map(|value| value.0),
            promotion_projection: self.promotion_projection.map(|value| value.0),
            tenant_lifecycle_state: self.tenant_lifecycle_state,
            installation_projection: self.installation_projection.map(|value| value.0),
            historical_authority_projection: self
                .historical_authority_projection
                .map(|value| value.0),
            current_authority_projection: self.current_authority_projection.map(|value| value.0),
            active_target_version: self.active_target_version,
            artifact_projection: self.artifact_projection.map(|value| value.0),
            attestation_projection: self.attestation_projection.map(|value| value.0),
            serving_projection: self.serving_projection.map(|value| value.0),
            database_now: self.database_now,
            attestation_evidence_v2: self.attestation_evidence_v2,
        })
    }
}

impl ProductDeploymentAttestationEvidenceV2 {
    fn is_empty(&self) -> bool {
        self.attestation_record_format_version.is_none()
            && self.attestation_serving_lease_duration_nanos.is_none()
            && self.attestation_convergence_attempt_no.is_none()
            && self.deployment_last_controller_id.is_none()
            && self.v2_evidence_state.is_none()
            && self.v2_operation_id.is_none()
            && self.v2_intent_fingerprint.is_none()
            && self.v2_certification_intent_bytes.is_none()
            && self.v2_request_digest.is_none()
            && self.v2_request_bytes.is_none()
            && self.v2_live_attestation_bytes.is_none()
            && self.v2_must_commit_before.is_none()
            && self.v2_route_admission.is_none()
            && self.v2_certified_snapshot.is_none()
    }

    pub(super) fn into_runtime(self) -> RuntimeDeploymentAttestationEvidenceV2 {
        RuntimeDeploymentAttestationEvidenceV2 {
            attestation_record_format_version: self.attestation_record_format_version,
            attestation_serving_lease_duration_nanos: self.attestation_serving_lease_duration_nanos,
            attestation_convergence_attempt_no: self.attestation_convergence_attempt_no,
            deployment_last_controller_id: self.deployment_last_controller_id,
            v2_evidence_state: self.v2_evidence_state,
            v2_operation_id: self.v2_operation_id,
            v2_intent_fingerprint: self.v2_intent_fingerprint,
            v2_certification_intent_bytes: self.v2_certification_intent_bytes,
            v2_request_digest: self.v2_request_digest,
            v2_request_bytes: self.v2_request_bytes,
            v2_live_attestation_bytes: self.v2_live_attestation_bytes,
            v2_must_commit_before: self.v2_must_commit_before,
            v2_route_admission: self.v2_route_admission.map(|value| value.0),
            v2_certified_snapshot: self.v2_certified_snapshot.map(|value| value.0),
        }
    }
}

fn validate_database_observation(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    database_now: DateTime<Utc>,
) -> Result<(), DeploymentStatusPortError> {
    let evidence = request.evidence();
    let maximum_lifetime =
        status_authority_lifetime(evidence.capability()).ok_or_else(indeterminate)?;
    if evidence.observed_at() > database_now
        || database_now >= evidence.expires_at()
        || evidence
            .observed_at()
            .checked_add_signed(maximum_lifetime)
            .is_none_or(|latest| evidence.expires_at() > latest)
    {
        return Err(indeterminate());
    }
    Ok(())
}

pub(super) fn status_authority_lifetime(capability: CapabilityV1) -> Option<chrono::Duration> {
    match capability {
        CapabilityV1::Read => Some(MAX_READ_AUTHORITY_LIFETIME),
        CapabilityV1::Apply => Some(MAX_APPLY_AUTHORITY_LIFETIME),
        CapabilityV1::Author
        | CapabilityV1::Promote
        | CapabilityV1::Approve
        | CapabilityV1::Reject
        | CapabilityV1::CancelLifecycle => None,
    }
}

pub(super) fn indeterminate() -> DeploymentStatusPortError {
    DeploymentStatusPortError::Indeterminate(
        "runtime deployment status projection is inconsistent".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_mismatch_row() -> ProductDeploymentStatusRow {
        ProductDeploymentStatusRow {
            request_outcome: "request_mismatch".to_string(),
            deployment_projection: None,
            activation_projection: None,
            promotion_projection: None,
            tenant_lifecycle_state: None,
            installation_projection: None,
            historical_authority_projection: None,
            current_authority_projection: None,
            active_target_version: None,
            artifact_projection: None,
            attestation_projection: None,
            serving_projection: None,
            database_now: Utc::now(),
            attestation_evidence_v2: ProductDeploymentAttestationEvidenceV2::default(),
        }
    }

    #[test]
    fn status_capabilities_have_distinct_freshness_bounds() {
        assert_eq!(
            status_authority_lifetime(CapabilityV1::Read),
            Some(chrono::Duration::seconds(30))
        );
        assert_eq!(
            status_authority_lifetime(CapabilityV1::Apply),
            Some(chrono::Duration::seconds(5))
        );
        assert_eq!(status_authority_lifetime(CapabilityV1::Promote), None);
        assert_eq!(status_authority_lifetime(CapabilityV1::Approve), None);
        assert_eq!(status_authority_lifetime(CapabilityV1::Reject), None);
        assert_eq!(
            status_authority_lifetime(CapabilityV1::CancelLifecycle),
            None
        );
    }

    #[test]
    fn request_mismatch_requires_every_attestation_sidecar_field_to_be_empty() {
        assert!(request_mismatch_row().sensitive_evidence_is_empty());

        macro_rules! assert_sensitive {
            ($field:ident, $value:expr) => {{
                let mut row = request_mismatch_row();
                row.attestation_evidence_v2.$field = Some($value);
                assert!(!row.sensitive_evidence_is_empty(), stringify!($field));
            }};
        }

        assert_sensitive!(attestation_record_format_version, 2);
        assert_sensitive!(attestation_serving_lease_duration_nanos, 1);
        assert_sensitive!(attestation_convergence_attempt_no, 1);
        assert_sensitive!(deployment_last_controller_id, "controller".to_string());
        assert_sensitive!(v2_evidence_state, "exact".to_string());
        assert_sensitive!(v2_operation_id, "0".repeat(32));
        assert_sensitive!(v2_intent_fingerprint, "1".repeat(64));
        assert_sensitive!(v2_certification_intent_bytes, vec![1]);
        assert_sensitive!(v2_request_digest, "2".repeat(64));
        assert_sensitive!(v2_request_bytes, vec![2]);
        assert_sensitive!(v2_live_attestation_bytes, vec![3]);
        assert_sensitive!(v2_must_commit_before, Utc::now());
        assert_sensitive!(v2_route_admission, Json(serde_json::json!({})));
        assert_sensitive!(v2_certified_snapshot, Json(serde_json::json!({})));
    }
}
