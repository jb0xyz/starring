use std::num::NonZeroU32;

use automation_runtime_convergence::{PromotionId, RuntimeDeploymentPhaseV1};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::artifact::{runtime_target_artifact_is_valid, RuntimeTargetArtifactRow};
use crate::model::{
    RuntimeAttestationObservationV2, RuntimeConvergenceAttemptV1, RuntimeDeploymentScopeV1,
    RuntimeDeploymentStatusV1, RuntimeDeploymentStatusV2, RuntimeDigestV1,
    RuntimeServingFreshnessV2, RuntimeServingObservationV2,
};
use crate::projection::{project_status, CurrentAuthorityOutcome, StatusProjectionEvidence};
use crate::row::{AttestationRow, DeploymentRow, PersistedDeployment, ServingLeaseRow};
use crate::RuntimeConvergenceStoreError;

pub struct RuntimeDeploymentStatusExpectationV1 {
    scope: RuntimeDeploymentScopeV1,
    promotion_id: PromotionId,
    desired_target_digest: RuntimeDigestV1,
    guild_id: String,
    discord_application_id: String,
    current_authority_revision: u64,
    current_authority_payload_digest: RuntimeDigestV1,
}

impl RuntimeDeploymentStatusExpectationV1 {
    pub fn new(
        scope: RuntimeDeploymentScopeV1,
        promotion_id: impl Into<String>,
        desired_target_digest: impl Into<String>,
        guild_id: impl Into<String>,
        discord_application_id: impl Into<String>,
        current_authority_revision: u64,
        current_authority_payload_digest: impl Into<String>,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        let promotion_id = PromotionId::parse(promotion_id.into()).map_err(|_| {
            RuntimeConvergenceStoreError::InvalidInput("runtime promotion identity")
        })?;
        let desired_target_digest = RuntimeDigestV1::parse(desired_target_digest.into())?;
        let guild_id = canonical_snowflake(guild_id.into(), "Discord guild identity")?;
        let discord_application_id = canonical_snowflake(
            discord_application_id.into(),
            "Discord application identity",
        )?;
        if current_authority_revision == 0 || i64::try_from(current_authority_revision).is_err() {
            return Err(RuntimeConvergenceStoreError::InvalidInput(
                "current installation authority revision",
            ));
        }
        let current_authority_payload_digest =
            RuntimeDigestV1::parse(current_authority_payload_digest.into())?;
        Ok(Self {
            scope,
            promotion_id,
            desired_target_digest,
            guild_id,
            discord_application_id,
            current_authority_revision,
            current_authority_payload_digest,
        })
    }

    pub fn scope(&self) -> &RuntimeDeploymentScopeV1 {
        &self.scope
    }
}

pub struct RuntimeDeploymentStatusEvidenceV1 {
    pub deployment_projection: Value,
    pub activation_projection: Option<Value>,
    pub promotion_projection: Option<Value>,
    pub tenant_lifecycle_state: Option<String>,
    pub installation_projection: Option<Value>,
    pub historical_authority_projection: Option<Value>,
    pub current_authority_projection: Option<Value>,
    pub active_target_version: Option<i64>,
    pub artifact_projection: Option<Value>,
    pub attestation_projection: Option<Value>,
    pub serving_projection: Option<Value>,
}

pub struct RuntimeDeploymentStatusEvidenceV2 {
    pub evidence: RuntimeDeploymentStatusEvidenceV1,
    pub deployment_convergence_attempt_no: i64,
    pub deployment_last_failure_attempt_no: Option<i64>,
    pub attestation_convergence_attempt_no: Option<i64>,
}

pub fn project_runtime_deployment_status_v2(
    expectation: &RuntimeDeploymentStatusExpectationV1,
    observed_at: DateTime<Utc>,
    mut evidence: RuntimeDeploymentStatusEvidenceV2,
) -> Result<RuntimeDeploymentStatusV2, RuntimeConvergenceStoreError> {
    let convergence_attempt = runtime_attempt(
        evidence.deployment_convergence_attempt_no,
        "runtime convergence attempt evidence",
    )?;
    let last_failure_attempt = evidence
        .deployment_last_failure_attempt_no
        .map(|value| positive_attempt(value, "runtime failure attempt evidence"))
        .transpose()?;
    let attestation_attempt = evidence
        .attestation_convergence_attempt_no
        .map(|value| positive_attempt(value, "runtime attestation attempt evidence"))
        .transpose()?;
    if evidence.evidence.attestation_projection.is_some() != attestation_attempt.is_some() {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "runtime attestation attempt presence",
        ));
    }
    attach_attempt_evidence(
        &mut evidence.evidence.deployment_projection,
        convergence_attempt,
        last_failure_attempt,
    )?;
    if let (Some(projection), Some(attempt)) = (
        evidence.evidence.attestation_projection.as_mut(),
        attestation_attempt,
    ) {
        attach_attestation_attempt_evidence(projection, attempt)?;
    }
    let serving_projection = evidence.evidence.serving_projection.clone();
    let status = project_runtime_deployment_status_v1(expectation, observed_at, evidence.evidence)?;
    let phase_live = matches!(status.snapshot.phase, RuntimeDeploymentPhaseV1::Live);
    if !phase_live && attestation_attempt.is_some()
        || attestation_attempt.is_some_and(|attempt| convergence_attempt.started() != Some(attempt))
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "runtime attestation attempt binding",
        ));
    }
    let freshness = serving_freshness(&status)?;
    let attestation = match freshness {
        RuntimeServingFreshnessV2::LeaseMissing
        | RuntimeServingFreshnessV2::IdentityMismatch
        | RuntimeServingFreshnessV2::Disconnected
        | RuntimeServingFreshnessV2::Expired
        | RuntimeServingFreshnessV2::Fresh => Some(RuntimeAttestationObservationV2 {
            deployment_revision: status.snapshot.revision,
            convergence_attempt: attestation_attempt.ok_or(
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime attestation attempt observation",
                ),
            )?,
        }),
        RuntimeServingFreshnessV2::NotExpected | RuntimeServingFreshnessV2::AttestationMissing => {
            None
        }
    };
    let serving_times = serving_times(&status, freshness, serving_projection)?;
    Ok(RuntimeDeploymentStatusV2 {
        status,
        convergence_attempt,
        last_failure_attempt,
        attestation,
        serving: RuntimeServingObservationV2 {
            freshness,
            last_heartbeat_at: serving_times.last_heartbeat_at,
            expires_at: serving_times.expires_at,
        },
    })
}

pub fn project_runtime_deployment_status_v1(
    expectation: &RuntimeDeploymentStatusExpectationV1,
    observed_at: DateTime<Utc>,
    evidence: RuntimeDeploymentStatusEvidenceV1,
) -> Result<RuntimeDeploymentStatusV1, RuntimeConvergenceStoreError> {
    let deployment = decode_envelope::<DeploymentRow>(
        evidence.deployment_projection,
        "deployment evidence envelope",
    )?
    .decode_legacy_evidence()?;
    validate_expected_deployment(expectation, &deployment)?;
    let installation = decode_optional_envelope::<InstallationEvidenceRow>(
        evidence.installation_projection,
        "installation evidence envelope",
    )?
    .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
        "installation authority evidence",
    ))?;
    let current_authority = decode_optional_envelope::<CurrentAuthorityEvidenceRow>(
        evidence.current_authority_projection,
        "current authority evidence envelope",
    )?
    .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
        "current installation authority evidence",
    ))?;
    validate_fresh_authority_binding(expectation, &installation, &current_authority)?;
    if matches!(
        deployment.deployment.snapshot().phase,
        RuntimeDeploymentPhaseV1::Cancelled { .. } | RuntimeDeploymentPhaseV1::Superseded { .. }
    ) {
        return project_status(
            expectation.scope(),
            observed_at,
            StatusProjectionEvidence {
                persisted: deployment,
                authority: CurrentAuthorityOutcome::NotEvaluated,
                attestation: None,
                serving: None,
            },
        );
    }
    let activation = decode_optional_envelope::<ActivationEvidenceRow>(
        evidence.activation_projection,
        "activation evidence envelope",
    )?;
    let promotion = decode_optional_envelope::<PromotionEvidenceRow>(
        evidence.promotion_projection,
        "promotion evidence envelope",
    )?;
    let historical_authority = decode_optional_envelope::<HistoricalAuthorityEvidenceRow>(
        evidence.historical_authority_projection,
        "historical authority evidence envelope",
    )?;
    let artifact = decode_optional_envelope::<RuntimeTargetArtifactRow>(
        evidence.artifact_projection,
        "RuleSet artifact evidence envelope",
    )?;
    let authority = evaluate_authority(
        &deployment,
        activation.as_ref(),
        promotion.as_ref(),
        evidence.tenant_lifecycle_state.as_deref(),
        &installation,
        historical_authority.as_ref(),
        &current_authority,
        evidence.active_target_version,
        artifact.as_ref(),
    )?;
    if authority != CurrentAuthorityOutcome::Exact
        || !matches!(
            deployment.deployment.snapshot().phase,
            RuntimeDeploymentPhaseV1::Live
        )
    {
        return project_status(
            expectation.scope(),
            observed_at,
            StatusProjectionEvidence {
                persisted: deployment,
                authority,
                attestation: None,
                serving: None,
            },
        );
    }
    let attestation = decode_optional_envelope::<AttestationRow>(
        evidence.attestation_projection,
        "attestation evidence envelope",
    )?
    .map(AttestationRow::decode_legacy_evidence)
    .transpose()?;
    let serving = decode_optional_envelope::<ServingLeaseRow>(
        evidence.serving_projection,
        "serving evidence envelope",
    )?;
    project_status(
        expectation.scope(),
        observed_at,
        StatusProjectionEvidence {
            persisted: deployment,
            authority,
            attestation,
            serving,
        },
    )
}

fn runtime_attempt(
    value: i64,
    error: &'static str,
) -> Result<RuntimeConvergenceAttemptV1, RuntimeConvergenceStoreError> {
    let value = u32::try_from(value)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidPersistedState(error))?;
    Ok(RuntimeConvergenceAttemptV1::new(value))
}

fn positive_attempt(
    value: i64,
    error: &'static str,
) -> Result<NonZeroU32, RuntimeConvergenceStoreError> {
    let value = u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(error))?;
    Ok(value)
}

fn attach_attempt_evidence(
    projection: &mut Value,
    convergence_attempt: RuntimeConvergenceAttemptV1,
    last_failure_attempt: Option<NonZeroU32>,
) -> Result<(), RuntimeConvergenceStoreError> {
    let row = evidence_row(projection, "deployment attempt evidence envelope")?;
    if row.contains_key("convergence_attempt_no") || row.contains_key("last_failure_attempt_no") {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "deployment attempt evidence fields",
        ));
    }
    insert_attempt(
        row,
        "convergence_attempt_no",
        i64::from(convergence_attempt.get()),
        "deployment convergence attempt evidence",
    )?;
    if let Some(attempt) = last_failure_attempt {
        insert_attempt(
            row,
            "last_failure_attempt_no",
            i64::from(attempt.get()),
            "deployment failure attempt evidence",
        )?;
    }
    Ok(())
}

fn attach_attestation_attempt_evidence(
    projection: &mut Value,
    attempt: NonZeroU32,
) -> Result<(), RuntimeConvergenceStoreError> {
    let row = evidence_row(projection, "attestation attempt evidence envelope")?;
    if row.contains_key("convergence_attempt_no") {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "attestation attempt evidence fields",
        ));
    }
    insert_attempt(
        row,
        "convergence_attempt_no",
        i64::from(attempt.get()),
        "attestation convergence attempt evidence",
    )
}

fn evidence_row<'a>(
    projection: &'a mut Value,
    error: &'static str,
) -> Result<&'a mut serde_json::Map<String, Value>, RuntimeConvergenceStoreError> {
    projection
        .as_object_mut()
        .and_then(|envelope| envelope.get_mut("row"))
        .and_then(Value::as_object_mut)
        .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(error))
}

fn insert_attempt(
    row: &mut serde_json::Map<String, Value>,
    key: &'static str,
    value: i64,
    error: &'static str,
) -> Result<(), RuntimeConvergenceStoreError> {
    if row.insert(key.to_string(), Value::from(value)).is_some() {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(error));
    }
    Ok(())
}

fn serving_freshness(
    status: &RuntimeDeploymentStatusV1,
) -> Result<RuntimeServingFreshnessV2, RuntimeConvergenceStoreError> {
    let freshness = match status.reason_code {
        "live" => RuntimeServingFreshnessV2::Fresh,
        "live_attestation_missing" => RuntimeServingFreshnessV2::AttestationMissing,
        "serving_lease_missing" => RuntimeServingFreshnessV2::LeaseMissing,
        "serving_identity_mismatch" => RuntimeServingFreshnessV2::IdentityMismatch,
        "gateway_not_serving" => RuntimeServingFreshnessV2::Disconnected,
        "serving_lease_expired" => RuntimeServingFreshnessV2::Expired,
        _ => RuntimeServingFreshnessV2::NotExpected,
    };
    match (freshness, &status.live) {
        (RuntimeServingFreshnessV2::Fresh, Some(_)) => Ok(freshness),
        (RuntimeServingFreshnessV2::Fresh, None) | (_, Some(_)) => {
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime serving freshness projection",
            ))
        }
        _ => Ok(freshness),
    }
}

struct ServingTimesV2 {
    last_heartbeat_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
}

fn serving_times(
    status: &RuntimeDeploymentStatusV1,
    freshness: RuntimeServingFreshnessV2,
    projection: Option<Value>,
) -> Result<ServingTimesV2, RuntimeConvergenceStoreError> {
    match freshness {
        RuntimeServingFreshnessV2::Fresh => {
            let live =
                status
                    .live
                    .as_ref()
                    .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                        "fresh runtime serving observation",
                    ))?;
            Ok(ServingTimesV2 {
                last_heartbeat_at: Some(live.last_heartbeat_at),
                expires_at: Some(live.expires_at),
            })
        }
        RuntimeServingFreshnessV2::Disconnected | RuntimeServingFreshnessV2::Expired => {
            let serving = decode_optional_envelope::<ServingLeaseRow>(
                projection,
                "serving freshness evidence envelope",
            )?
            .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                "serving freshness evidence",
            ))?;
            serving.validate()?;
            Ok(ServingTimesV2 {
                last_heartbeat_at: Some(serving.last_heartbeat_at),
                expires_at: Some(serving.expires_at),
            })
        }
        RuntimeServingFreshnessV2::NotExpected
        | RuntimeServingFreshnessV2::AttestationMissing
        | RuntimeServingFreshnessV2::LeaseMissing
        | RuntimeServingFreshnessV2::IdentityMismatch => Ok(ServingTimesV2 {
            last_heartbeat_at: None,
            expires_at: None,
        }),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceEnvelope<T> {
    evidence_format_version: u16,
    row: T,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationEvidenceRow {
    id: String,
    tenant_id: Option<String>,
    installation_id: Option<String>,
    guild_id: String,
    ruleset_key: String,
    target_version: i64,
    target_content_hash: String,
    state: String,
    authority_kind: String,
    link_state_name: String,
    promotion_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PromotionEvidenceRow {
    id: String,
    stage: String,
    tenant_id: String,
    installation_id: String,
    record_authority_tenant_id: String,
    record_authority_installation_id: String,
    record_authority_guild_id: String,
    record_authority_ruleset_key: String,
    record_authority_binding_revision: String,
    record_context_fingerprint: String,
    record_activation_request_id: String,
    record_activation_guild_id: String,
    record_activation_ruleset_key: String,
    record_activation_target_version: String,
    record_activation_target_content_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallationEvidenceRow {
    installation_id: String,
    tenant_id: String,
    discord_application_id: String,
    discord_guild_id: String,
    ruleset_key: String,
    lifecycle_state: String,
    current_authority_revision: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalAuthorityEvidenceRow {
    installation_id: String,
    tenant_id: String,
    revision: i64,
    binding_revision: i64,
    resource_bindings: Value,
    binding_fingerprint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentAuthorityEvidenceRow {
    installation_id: String,
    tenant_id: String,
    revision: i64,
    binding_revision: i64,
    resource_bindings: Value,
    binding_fingerprint: String,
    authority_payload_digest: String,
}

fn decode_envelope<T: DeserializeOwned>(
    value: Value,
    error: &'static str,
) -> Result<T, RuntimeConvergenceStoreError> {
    let envelope = serde_json::from_value::<EvidenceEnvelope<T>>(value)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidPersistedState(error))?;
    if envelope.evidence_format_version != 1 {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(error));
    }
    Ok(envelope.row)
}

fn decode_optional_envelope<T: DeserializeOwned>(
    value: Option<Value>,
    error: &'static str,
) -> Result<Option<T>, RuntimeConvergenceStoreError> {
    value.map(|value| decode_envelope(value, error)).transpose()
}

fn validate_expected_deployment(
    expectation: &RuntimeDeploymentStatusExpectationV1,
    deployment: &PersistedDeployment,
) -> Result<(), RuntimeConvergenceStoreError> {
    let snapshot = deployment.deployment.snapshot();
    if snapshot.identity.tenant_id != expectation.scope.tenant_id
        || snapshot.identity.installation_id != expectation.scope.installation_id
        || snapshot.identity.deployment_id != expectation.scope.deployment_id
        || snapshot.identity.promotion_id != expectation.promotion_id
        || snapshot.target.guild_id.to_string() != expectation.guild_id
        || deployment.desired_target_digest != expectation.desired_target_digest
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "deployment expectation binding",
        ));
    }
    Ok(())
}

fn validate_fresh_authority_binding(
    expectation: &RuntimeDeploymentStatusExpectationV1,
    installation: &InstallationEvidenceRow,
    current_authority: &CurrentAuthorityEvidenceRow,
) -> Result<(), RuntimeConvergenceStoreError> {
    let revision = i64::try_from(expectation.current_authority_revision).map_err(|_| {
        RuntimeConvergenceStoreError::InvalidPersistedState("fresh authority revision")
    })?;
    if installation.current_authority_revision <= 0
        || current_authority.revision <= 0
        || current_authority.binding_revision <= 0
        || !current_authority.resource_bindings.is_object()
        || serde_json::to_vec(&current_authority.resource_bindings)
            .map_or(true, |encoded| encoded.len() > 262_144)
        || RuntimeDigestV1::parse(current_authority.binding_fingerprint.clone()).is_err()
        || RuntimeDigestV1::parse(current_authority.authority_payload_digest.clone()).is_err()
        || installation.tenant_id != expectation.scope.tenant_id.as_str()
        || installation.installation_id != expectation.scope.installation_id.as_str()
        || installation.discord_application_id != expectation.discord_application_id
        || installation.discord_guild_id != expectation.guild_id
        || installation.current_authority_revision != revision
        || current_authority.tenant_id != expectation.scope.tenant_id.as_str()
        || current_authority.installation_id != expectation.scope.installation_id.as_str()
        || current_authority.revision != revision
        || current_authority.authority_payload_digest
            != expectation.current_authority_payload_digest.as_str()
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "fresh authority evidence binding",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn evaluate_authority(
    deployment: &PersistedDeployment,
    activation: Option<&ActivationEvidenceRow>,
    promotion: Option<&PromotionEvidenceRow>,
    tenant_lifecycle_state: Option<&str>,
    installation: &InstallationEvidenceRow,
    historical_authority: Option<&HistoricalAuthorityEvidenceRow>,
    current_authority: &CurrentAuthorityEvidenceRow,
    active_target_version: Option<i64>,
    artifact: Option<&RuntimeTargetArtifactRow>,
) -> Result<CurrentAuthorityOutcome, RuntimeConvergenceStoreError> {
    let snapshot = deployment.deployment.snapshot();
    if matches!(
        snapshot.phase,
        RuntimeDeploymentPhaseV1::Cancelled { .. } | RuntimeDeploymentPhaseV1::Superseded { .. }
    ) {
        return Ok(CurrentAuthorityOutcome::NotEvaluated);
    }
    let identity = &snapshot.identity;
    let target = &snapshot.target;
    let target_version = i64::from(target.version.get());
    let binding_revision = i64::try_from(target.binding_revision.get()).map_err(|_| {
        RuntimeConvergenceStoreError::InvalidPersistedState("target binding revision")
    })?;
    let authority_revision = i64::try_from(deployment.installation_authority_revision)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidPersistedState("authority revision"))?;
    let activation_exact = activation.is_some_and(|row| {
        row.id == identity.activation_request_id.as_str()
            && row.authority_kind == "product_authoring"
            && row.link_state_name == "linked"
            && row.state == "applied"
            && row.promotion_id.as_deref() == Some(identity.promotion_id.as_str())
            && row.tenant_id.as_deref() == Some(identity.tenant_id.as_str())
            && row.installation_id.as_deref() == Some(identity.installation_id.as_str())
            && row.guild_id == target.guild_id.to_string()
            && row.ruleset_key == target.ruleset_key.as_str()
            && row.target_version == target_version
            && row.target_content_hash == target.content_hash.to_hex()
    });
    if !activation_exact {
        return Ok(CurrentAuthorityOutcome::ScopeMismatch);
    }
    let promotion_exact = promotion.is_some_and(|row| {
        row.id == identity.promotion_id.as_str()
            && row.stage == "activation_pending"
            && row.tenant_id == identity.tenant_id.as_str()
            && row.installation_id == identity.installation_id.as_str()
            && row.record_authority_tenant_id == identity.tenant_id.as_str()
            && row.record_authority_installation_id == identity.installation_id.as_str()
            && row.record_authority_guild_id == target.guild_id.to_string()
            && row.record_authority_ruleset_key == target.ruleset_key.as_str()
            && parse_positive_i64(&row.record_authority_binding_revision) == Some(binding_revision)
            && row.record_context_fingerprint == target.binding_fingerprint.as_str()
            && row.record_activation_request_id == identity.activation_request_id.as_str()
            && row.record_activation_guild_id == target.guild_id.to_string()
            && row.record_activation_ruleset_key == target.ruleset_key.as_str()
            && parse_positive_i64(&row.record_activation_target_version) == Some(target_version)
            && row.record_activation_target_content_hash == target.content_hash.to_hex()
    });
    if !promotion_exact {
        return Ok(CurrentAuthorityOutcome::ScopeMismatch);
    }
    if tenant_lifecycle_state != Some("active") {
        return Ok(CurrentAuthorityOutcome::LifecycleInactive);
    }
    if installation.tenant_id != identity.tenant_id.as_str()
        || installation.installation_id != identity.installation_id.as_str()
        || installation.discord_guild_id != target.guild_id.to_string()
        || installation.ruleset_key != target.ruleset_key.as_str()
    {
        return Ok(CurrentAuthorityOutcome::ScopeMismatch);
    }
    if installation.lifecycle_state != "active" {
        return Ok(CurrentAuthorityOutcome::LifecycleInactive);
    }
    let historical_exact = historical_authority.is_some_and(|row| {
        row.revision > 0
            && row.binding_revision > 0
            && row.resource_bindings.is_object()
            && serde_json::to_vec(&row.resource_bindings)
                .is_ok_and(|encoded| encoded.len() <= 262_144)
            && RuntimeDigestV1::parse(row.binding_fingerprint.clone()).is_ok()
            && row.tenant_id == identity.tenant_id.as_str()
            && row.installation_id == identity.installation_id.as_str()
            && row.revision == authority_revision
            && row.binding_revision == binding_revision
            && row.binding_fingerprint == target.binding_fingerprint.as_str()
    });
    if !historical_exact {
        return Ok(CurrentAuthorityOutcome::BindingMismatch);
    }
    let historical_authority = historical_authority.ok_or(
        RuntimeConvergenceStoreError::InvalidPersistedState("historical authority evidence"),
    )?;
    let current_exact = current_authority.tenant_id == identity.tenant_id.as_str()
        && current_authority.installation_id == identity.installation_id.as_str()
        && current_authority.revision == installation.current_authority_revision
        && current_authority.binding_revision == binding_revision
        && current_authority.binding_fingerprint == target.binding_fingerprint.as_str()
        && current_authority.resource_bindings == historical_authority.resource_bindings;
    if !current_exact {
        return Ok(CurrentAuthorityOutcome::BindingMismatch);
    }
    if active_target_version != Some(target_version) {
        return Ok(CurrentAuthorityOutcome::ActiveMismatch);
    }
    let Some(artifact) = artifact else {
        return Ok(CurrentAuthorityOutcome::ActiveMismatch);
    };
    if artifact.content_hash != target.content_hash.to_hex() {
        return Ok(CurrentAuthorityOutcome::ActiveMismatch);
    }
    if !runtime_target_artifact_is_valid(artifact, &target.content_hash) {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "RuleSet artifact integrity",
        ));
    }
    Ok(CurrentAuthorityOutcome::Exact)
}

fn canonical_snowflake(
    value: String,
    field: &'static str,
) -> Result<String, RuntimeConvergenceStoreError> {
    let parsed = value
        .parse::<u64>()
        .ok()
        .filter(|parsed| *parsed > 0)
        .ok_or(RuntimeConvergenceStoreError::InvalidInput(field))?;
    if parsed.to_string() != value {
        return Err(RuntimeConvergenceStoreError::InvalidInput(field));
    }
    Ok(value)
}

fn parse_positive_i64(value: &str) -> Option<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|parsed| *parsed > 0 && parsed.to_string() == value)
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{content_hash, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_runtime_convergence::{
        ActivationRequestId, BindingRevision, CommandGuardV1, ControllerId, DeploymentId,
        DeploymentRevision, FencingToken, InstallationId, LeaseRequestV1, PromotionId,
        RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentSnapshotV1,
        RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
    };
    use automation_state::InteractionRuleSet;
    use chrono::DateTime;
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;
    use serde::Deserialize;
    use serde_json::{json, Value};

    use super::{
        canonical_snowflake, decode_envelope, parse_positive_i64,
        project_runtime_deployment_status_v1, project_runtime_deployment_status_v2,
        RuntimeDeploymentStatusEvidenceV1, RuntimeDeploymentStatusEvidenceV2,
        RuntimeDeploymentStatusExpectationV1,
    };
    use crate::{
        prepare_requested_deployment_v1, DeploymentAvailabilityV1, EnqueueDeploymentV1,
        RuntimeConvergenceStoreError, RuntimeDeploymentScopeV1, RuntimeServingFreshnessV2,
    };

    #[derive(Deserialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct TestRow {
        value: String,
    }

    fn envelope(row: Value) -> Value {
        json!({"evidence_format_version": 1, "row": row})
    }

    fn pending_fixture() -> (
        RuntimeDeploymentStatusExpectationV1,
        RuntimeDeploymentStatusEvidenceV1,
        DateTime<chrono::Utc>,
    ) {
        let observed_at = DateTime::from_timestamp(1_700_000_001, 0).unwrap();
        let requested_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let definition = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        };
        let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
        let promotion_id = "a".repeat(64);
        let binding_fingerprint = "c".repeat(64);
        let authority_payload_digest = "d".repeat(64);
        let request = EnqueueDeploymentV1 {
            identity: RuntimeDeploymentIdentityV1 {
                deployment_id: DeploymentId::parse("status-deployment").unwrap(),
                tenant_id: TenantId::parse("status-tenant").unwrap(),
                installation_id: InstallationId::parse("status-installation").unwrap(),
                promotion_id: PromotionId::parse(&promotion_id).unwrap(),
                activation_request_id: ActivationRequestId::parse("status_activation").unwrap(),
            },
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(9_200_101),
                ruleset_key: "status_ruleset".parse().unwrap(),
                version: RuleSetVersionId::FIRST,
                content_hash,
                binding_revision: BindingRevision::FIRST,
                binding_fingerprint: ResourceBindingFingerprint::parse(&binding_fingerprint)
                    .unwrap(),
            },
            runtime_generation: RuntimeGeneration::FIRST,
            previous_runtime: None,
            installation_authority_revision: 1,
        };
        let prepared = prepare_requested_deployment_v1(request, requested_at).unwrap();
        let desired_target_digest = prepared.desired_target_digest().to_string();
        let content_hash = content_hash.to_hex();
        let expectation = RuntimeDeploymentStatusExpectationV1::new(
            RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("status-tenant").unwrap(),
                installation_id: InstallationId::parse("status-installation").unwrap(),
                deployment_id: DeploymentId::parse("status-deployment").unwrap(),
            },
            promotion_id.clone(),
            desired_target_digest.clone(),
            "9200101",
            "9200102",
            1,
            authority_payload_digest.clone(),
        )
        .unwrap();
        let evidence = RuntimeDeploymentStatusEvidenceV1 {
            deployment_projection: envelope(json!({
                "deployment_id": "status-deployment",
                "tenant_id": "status-tenant",
                "installation_id": "status-installation",
                "promotion_id": promotion_id,
                "activation_request_id": "status_activation",
                "installation_authority_revision": 1,
                "guild_id": "9200101",
                "ruleset_key": "status_ruleset",
                "target_version": 1,
                "target_content_hash": content_hash,
                "binding_revision": 1,
                "binding_fingerprint": binding_fingerprint,
                "desired_target_digest": desired_target_digest,
                "runtime_generation": 1,
                "previous_runtime": null,
                "requested_at": requested_at,
                "snapshot_format_version": 1,
                "snapshot": prepared.snapshot_json(),
                "revision": 1,
                "phase": "requested",
                "controller_id": null,
                "controller_fencing_token": null,
                "controller_acquired_at": null,
                "controller_lease_expires_at": null,
                "last_fencing_token": null,
                "next_retry_at": null,
                "last_stable_error_code": null,
                "live_attestation_id": null,
                "live_at": null,
                "blocked_at": null,
                "superseded_at": null,
                "cancelled_at": null,
                "created_at": requested_at,
                "updated_at": requested_at
            })),
            activation_projection: Some(envelope(json!({
                "id": "status_activation",
                "tenant_id": "status-tenant",
                "installation_id": "status-installation",
                "guild_id": "9200101",
                "ruleset_key": "status_ruleset",
                "target_version": 1,
                "target_content_hash": content_hash,
                "state": "applied",
                "authority_kind": "product_authoring",
                "link_state_name": "linked",
                "promotion_id": promotion_id
            }))),
            promotion_projection: Some(envelope(json!({
                "id": promotion_id,
                "stage": "activation_pending",
                "tenant_id": "status-tenant",
                "installation_id": "status-installation",
                "record_authority_tenant_id": "status-tenant",
                "record_authority_installation_id": "status-installation",
                "record_authority_guild_id": "9200101",
                "record_authority_ruleset_key": "status_ruleset",
                "record_authority_binding_revision": "1",
                "record_context_fingerprint": binding_fingerprint,
                "record_activation_request_id": "status_activation",
                "record_activation_guild_id": "9200101",
                "record_activation_ruleset_key": "status_ruleset",
                "record_activation_target_version": "1",
                "record_activation_target_content_hash": content_hash
            }))),
            tenant_lifecycle_state: Some("active".into()),
            installation_projection: Some(envelope(json!({
                "installation_id": "status-installation",
                "tenant_id": "status-tenant",
                "discord_application_id": "9200102",
                "discord_guild_id": "9200101",
                "ruleset_key": "status_ruleset",
                "lifecycle_state": "active",
                "current_authority_revision": 1
            }))),
            historical_authority_projection: Some(envelope(json!({
                "installation_id": "status-installation",
                "tenant_id": "status-tenant",
                "revision": 1,
                "binding_revision": 1,
                "resource_bindings": {},
                "binding_fingerprint": binding_fingerprint
            }))),
            current_authority_projection: Some(envelope(json!({
                "installation_id": "status-installation",
                "tenant_id": "status-tenant",
                "revision": 1,
                "binding_revision": 1,
                "resource_bindings": {},
                "binding_fingerprint": binding_fingerprint,
                "authority_payload_digest": authority_payload_digest
            }))),
            active_target_version: Some(1),
            artifact_projection: Some(envelope(json!({
                "schema_version": CURRENT_RULESET_SCHEMA_VERSION.get(),
                "definition": definition,
                "content_hash": content_hash,
                "canonical_content_hash": content_hash
            }))),
            attestation_projection: None,
            serving_projection: None,
        };
        (expectation, evidence, observed_at)
    }

    fn make_cancelled(evidence: &mut RuntimeDeploymentStatusEvidenceV1) {
        let requested_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let cancelled_at = DateTime::from_timestamp(1_700_000_001, 0).unwrap();
        let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(
            evidence.deployment_projection["row"]["snapshot"].clone(),
        )
        .unwrap();
        let mut deployment = RuntimeDeployment::restore(snapshot).unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: DeploymentRevision::FIRST,
                controller_id: ControllerId::parse("status-controller").unwrap(),
                fencing_token: FencingToken::FIRST,
                now: requested_at,
                expires_at: DateTime::from_timestamp(1_700_000_060, 0).unwrap(),
            })
            .unwrap();
        deployment
            .cancel(
                &CommandGuardV1 {
                    expected_revision: deployment.revision(),
                    controller_id: ControllerId::parse("status-controller").unwrap(),
                    fencing_token: FencingToken::FIRST,
                    runtime_generation: RuntimeGeneration::FIRST,
                    now: cancelled_at,
                },
                "product_cancelled".into(),
                cancelled_at,
            )
            .unwrap();
        let snapshot = deployment.snapshot();
        let row = &mut evidence.deployment_projection["row"];
        row["snapshot"] = serde_json::to_value(&snapshot).unwrap();
        row["revision"] = json!(snapshot.revision.get());
        row["phase"] = json!("cancelled");
        row["controller_id"] = Value::Null;
        row["controller_fencing_token"] = Value::Null;
        row["controller_acquired_at"] = Value::Null;
        row["controller_lease_expires_at"] = Value::Null;
        row["last_fencing_token"] = json!(1);
        row["cancelled_at"] = json!(cancelled_at);
        row["updated_at"] = json!(cancelled_at);
    }

    #[test]
    fn evidence_envelopes_are_versioned_and_closed() {
        let decoded = decode_envelope::<TestRow>(
            json!({"evidence_format_version": 1, "row": {"value": "ok"}}),
            "test evidence",
        )
        .unwrap();
        assert!(decoded == TestRow { value: "ok".into() });
        for value in [
            json!({"evidence_format_version": 2, "row": {"value": "ok"}}),
            json!({"evidence_format_version": 1, "row": {"value": "ok", "extra": true}}),
            json!({"evidence_format_version": 1, "row": {"value": "ok"}, "extra": true}),
        ] {
            assert!(matches!(
                decode_envelope::<TestRow>(value, "test evidence"),
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "test evidence"
                ))
            ));
        }
    }

    #[test]
    fn database_number_text_requires_canonical_positive_decimal() {
        assert_eq!(parse_positive_i64("1"), Some(1));
        assert_eq!(parse_positive_i64("9223372036854775807"), Some(i64::MAX));
        assert_eq!(parse_positive_i64("01"), None);
        assert_eq!(parse_positive_i64("0"), None);
        assert_eq!(parse_positive_i64("-1"), None);
        assert_eq!(parse_positive_i64("9223372036854775808"), None);
    }

    #[test]
    fn snowflake_expectations_require_canonical_nonzero_u64() {
        assert_eq!(canonical_snowflake("1".into(), "snowflake").unwrap(), "1");
        assert_eq!(
            canonical_snowflake(u64::MAX.to_string(), "snowflake").unwrap(),
            u64::MAX.to_string()
        );
        for value in ["0", "01", "-1", "18446744073709551616"] {
            assert!(matches!(
                canonical_snowflake(value.into(), "snowflake"),
                Err(RuntimeConvergenceStoreError::InvalidInput("snowflake"))
            ));
        }
    }

    #[test]
    fn raw_evidence_reuses_the_runtime_pending_projector() {
        let (expectation, evidence, observed_at) = pending_fixture();
        let status =
            project_runtime_deployment_status_v1(&expectation, observed_at, evidence).unwrap();
        assert_eq!(
            status.availability,
            DeploymentAvailabilityV1::RuntimePending
        );
        assert_eq!(status.reason_code, "convergence_in_progress");
        assert_eq!(status.observed_at, observed_at);
        assert!(status.live.is_none());
    }

    #[test]
    fn operational_evidence_accepts_only_exact_pristine_attempt_zero() {
        let (expectation, evidence, observed_at) = pending_fixture();
        let status = project_runtime_deployment_status_v2(
            &expectation,
            observed_at,
            RuntimeDeploymentStatusEvidenceV2 {
                evidence,
                deployment_convergence_attempt_no: 0,
                deployment_last_failure_attempt_no: None,
                attestation_convergence_attempt_no: None,
            },
        )
        .unwrap();
        assert_eq!(status.convergence_attempt.get(), 0);
        assert_eq!(status.last_failure_attempt, None);
        assert_eq!(status.attestation, None);
        assert_eq!(
            status.serving.freshness,
            RuntimeServingFreshnessV2::NotExpected
        );
        assert_eq!(status.serving.last_heartbeat_at, None);
        assert_eq!(status.serving.expires_at, None);
        assert_eq!(
            status.status.availability,
            DeploymentAvailabilityV1::RuntimePending
        );
    }

    #[test]
    fn operational_attempt_scalars_fail_closed() {
        for current in [-1, i64::from(u32::MAX) + 1] {
            let (expectation, evidence, observed_at) = pending_fixture();
            assert!(matches!(
                project_runtime_deployment_status_v2(
                    &expectation,
                    observed_at,
                    RuntimeDeploymentStatusEvidenceV2 {
                        evidence,
                        deployment_convergence_attempt_no: current,
                        deployment_last_failure_attempt_no: None,
                        attestation_convergence_attempt_no: None,
                    },
                ),
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(_))
            ));
        }
        let (expectation, evidence, observed_at) = pending_fixture();
        assert!(matches!(
            project_runtime_deployment_status_v2(
                &expectation,
                observed_at,
                RuntimeDeploymentStatusEvidenceV2 {
                    evidence,
                    deployment_convergence_attempt_no: 0,
                    deployment_last_failure_attempt_no: Some(1),
                    attestation_convergence_attempt_no: None,
                },
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(_))
        ));
        let (expectation, evidence, observed_at) = pending_fixture();
        assert!(matches!(
            project_runtime_deployment_status_v2(
                &expectation,
                observed_at,
                RuntimeDeploymentStatusEvidenceV2 {
                    evidence,
                    deployment_convergence_attempt_no: 0,
                    deployment_last_failure_attempt_no: None,
                    attestation_convergence_attempt_no: Some(1),
                },
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime attestation attempt presence"
            ))
        ));
    }

    #[test]
    fn operational_evidence_rejects_attempt_fields_inside_the_v1_payload() {
        let (expectation, mut evidence, observed_at) = pending_fixture();
        evidence.deployment_projection["row"]["convergence_attempt_no"] = json!(0);
        assert!(matches!(
            project_runtime_deployment_status_v2(
                &expectation,
                observed_at,
                RuntimeDeploymentStatusEvidenceV2 {
                    evidence,
                    deployment_convergence_attempt_no: 0,
                    deployment_last_failure_attempt_no: None,
                    attestation_convergence_attempt_no: None,
                },
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "deployment attempt evidence fields"
            ))
        ));
    }

    #[test]
    fn authority_evidence_rejects_schema_expansion() {
        let (expectation, mut evidence, observed_at) = pending_fixture();
        evidence.activation_projection.as_mut().unwrap()["row"]["unexpected"] = json!(true);
        assert!(matches!(
            project_runtime_deployment_status_v1(&expectation, observed_at, evidence),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "activation evidence envelope"
            ))
        ));
    }

    #[test]
    fn fresh_authority_digest_is_mandatory() {
        let (_, evidence, observed_at) = pending_fixture();
        let expectation = RuntimeDeploymentStatusExpectationV1::new(
            RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("status-tenant").unwrap(),
                installation_id: InstallationId::parse("status-installation").unwrap(),
                deployment_id: DeploymentId::parse("status-deployment").unwrap(),
            },
            "a".repeat(64),
            evidence.deployment_projection["row"]["desired_target_digest"]
                .as_str()
                .unwrap(),
            "9200101",
            "9200102",
            1,
            "e".repeat(64),
        )
        .unwrap();
        assert!(matches!(
            project_runtime_deployment_status_v1(&expectation, observed_at, evidence),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "fresh authority evidence binding"
            ))
        ));
    }

    #[test]
    fn fresh_authority_identity_is_mandatory() {
        for field in [
            "discord_application_id",
            "discord_guild_id",
            "current_authority_revision",
        ] {
            let (expectation, mut evidence, observed_at) = pending_fixture();
            evidence.installation_projection.as_mut().unwrap()["row"][field] = match field {
                "discord_application_id" => json!("9200103"),
                "discord_guild_id" => json!("9200104"),
                "current_authority_revision" => json!(2),
                _ => unreachable!(),
            };
            assert!(matches!(
                project_runtime_deployment_status_v1(&expectation, observed_at, evidence),
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "fresh authority evidence binding"
                ))
            ));
        }
        for field in ["revision", "authority_payload_digest"] {
            let (expectation, mut evidence, observed_at) = pending_fixture();
            evidence.current_authority_projection.as_mut().unwrap()["row"][field] = match field {
                "revision" => json!(2),
                "authority_payload_digest" => json!("e".repeat(64)),
                _ => unreachable!(),
            };
            assert!(matches!(
                project_runtime_deployment_status_v1(&expectation, observed_at, evidence),
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "fresh authority evidence binding"
                ))
            ));
        }
    }

    #[test]
    fn terminal_status_still_requires_fresh_access_binding() {
        let (expectation, mut evidence, observed_at) = pending_fixture();
        make_cancelled(&mut evidence);
        evidence.installation_projection.as_mut().unwrap()["row"]["discord_application_id"] =
            json!("9200103");
        assert!(matches!(
            project_runtime_deployment_status_v1(&expectation, observed_at, evidence),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "fresh authority evidence binding"
            ))
        ));
    }

    #[test]
    fn non_live_status_ignores_unrequested_live_evidence() {
        let (expectation, mut evidence, observed_at) = pending_fixture();
        evidence.attestation_projection = Some(json!({"not": "an envelope"}));
        evidence.serving_projection = Some(json!({"not": "an envelope"}));
        let status =
            project_runtime_deployment_status_v1(&expectation, observed_at, evidence).unwrap();
        assert_eq!(
            status.availability,
            DeploymentAvailabilityV1::RuntimePending
        );
        assert_eq!(status.reason_code, "convergence_in_progress");
        assert!(status.live.is_none());
    }

    #[test]
    fn terminal_status_ignores_later_runtime_authority_and_live_evidence() {
        let (expectation, mut evidence, observed_at) = pending_fixture();
        make_cancelled(&mut evidence);
        evidence.activation_projection = Some(json!({"not": "an envelope"}));
        evidence.promotion_projection = Some(json!({"not": "an envelope"}));
        evidence.historical_authority_projection = Some(json!({"not": "an envelope"}));
        evidence.artifact_projection = Some(json!({"not": "an envelope"}));
        evidence.attestation_projection = Some(json!({"not": "an envelope"}));
        evidence.serving_projection = Some(json!({"not": "an envelope"}));
        let status =
            project_runtime_deployment_status_v1(&expectation, observed_at, evidence).unwrap();
        assert_eq!(status.availability, DeploymentAvailabilityV1::Cancelled);
        assert_eq!(status.reason_code, "deployment_cancelled");
        assert!(status.live.is_none());
    }
}
