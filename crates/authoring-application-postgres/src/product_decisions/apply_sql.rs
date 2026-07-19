use authoring_application::{AuthorizedApplyProductV1, FreshGuildAuthorityEvidence};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use super::apply_contract::{FINALIZE_QUERY, LOCK_QUERY, TARGET_ARTIFACT_QUERY};
use super::apply_projection::PreparedProductApplyV1;
use super::digest::ApplyDigests;

#[derive(sqlx::FromRow)]
pub(super) struct ApplyLockRow {
    pub outcome: String,
    pub exact_replay: bool,
    pub requires_commit: bool,
    pub resulting_revision: Option<i64>,
    pub resulting_state: Option<String>,
    pub deployment_id: Option<String>,
    pub desired_target_digest: Option<String>,
    pub locked_projection: Option<Json<Value>>,
}

#[derive(sqlx::FromRow)]
pub(super) struct ApplyFinalizeRow {
    pub outcome: String,
    pub resulting_revision: Option<i64>,
    pub resulting_state: Option<String>,
    pub exact_replay: bool,
    pub guild_id: Option<String>,
    pub deployment_id: Option<String>,
    pub desired_target_digest: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(super) struct ApplyTargetArtifactRow {
    pub schema_version: i64,
    pub definition: Option<Json<Value>>,
    pub content_hash: String,
    pub canonical_content_hash: Option<String>,
}

pub(super) async fn load_apply_target_artifact(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<Vec<ApplyTargetArtifactRow>, sqlx::Error> {
    sqlx::query_as::<_, ApplyTargetArtifactRow>(TARGET_ARTIFACT_QUERY)
        .bind(request.scope().tenant_id().as_str())
        .bind(request.scope().installation_id().as_str())
        .bind(request.command().promotion.promotion_id().as_str())
        .bind(request.actor().principal_id().as_str())
        .bind(request.session_fingerprint().as_bytes().as_slice())
        .bind(request.scope().acting_user_id().to_string())
        .bind(request.scope().guild_id().to_string())
        .fetch_all(&mut **transaction)
        .await
}

pub(super) async fn lock_apply(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    expected_revision: i64,
    authority_revision: i64,
) -> Result<ApplyLockRow, sqlx::Error> {
    let evidence = request.evidence();
    sqlx::query_as::<_, ApplyLockRow>(LOCK_QUERY)
        .bind(request.scope().tenant_id().as_str())
        .bind(request.scope().installation_id().as_str())
        .bind(request.command().promotion.promotion_id().as_str())
        .bind(expected_revision)
        .bind(request.command().expected_payload_digest.as_str())
        .bind(request.actor().principal_id().as_str())
        .bind(request.session_fingerprint().as_bytes().as_slice())
        .bind(&digests.session_subject)
        .bind(evidence.acting_user_id().to_string())
        .bind(evidence.discord_application_id().get().to_string())
        .bind(evidence.guild_id().to_string())
        .bind("apply")
        .bind(authority_revision)
        .bind(evidence.installation_authority_digest())
        .bind(evidence.observation_digest())
        .bind(evidence.observed_at())
        .bind(evidence.expires_at())
        .bind(evidence.effective_permissions_bits().to_string())
        .bind(evidence.guild_owner())
        .bind(request.request_id().as_str())
        .bind(&digests.active_idempotency)
        .bind(&digests.idempotency_candidates)
        .bind(&digests.idempotency_candidate_key_ids)
        .bind(&digests.idempotency_candidate_key_fingerprints)
        .bind(&digests.active_key_id)
        .bind(&digests.semantic_request)
        .bind(&digests.receipt_id)
        .bind(&digests.audit_event_id)
        .bind(&digests.apply_attempt_id)
        .bind(&digests.deployment_id)
        .fetch_one(&mut **transaction)
        .await
}

pub(super) async fn finalize_apply(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    expected_revision: i64,
    authority_revision: i64,
    locked_projection: &Value,
    prepared: &PreparedProductApplyV1,
) -> Result<ApplyFinalizeRow, sqlx::Error> {
    let evidence = request.evidence();
    let previous_runtime = prepared
        .deployment
        .previous_runtime_json()
        .cloned()
        .unwrap_or(Value::Null);
    sqlx::query_as::<_, ApplyFinalizeRow>(FINALIZE_QUERY)
        .bind(request.scope().tenant_id().as_str())
        .bind(request.scope().installation_id().as_str())
        .bind(request.command().promotion.promotion_id().as_str())
        .bind(expected_revision)
        .bind(request.command().expected_payload_digest.as_str())
        .bind(request.actor().principal_id().as_str())
        .bind(request.session_fingerprint().as_bytes().as_slice())
        .bind(&digests.session_subject)
        .bind(evidence.acting_user_id().to_string())
        .bind(evidence.discord_application_id().get().to_string())
        .bind(evidence.guild_id().to_string())
        .bind("apply")
        .bind(authority_revision)
        .bind(evidence.installation_authority_digest())
        .bind(evidence.observation_digest())
        .bind(evidence.observed_at())
        .bind(evidence.expires_at())
        .bind(evidence.effective_permissions_bits().to_string())
        .bind(evidence.guild_owner())
        .bind(request.request_id().as_str())
        .bind(&digests.active_idempotency)
        .bind(&digests.idempotency_candidates)
        .bind(&digests.idempotency_candidate_key_ids)
        .bind(&digests.idempotency_candidate_key_fingerprints)
        .bind(&digests.active_key_id)
        .bind(&digests.semantic_request)
        .bind(&digests.receipt_id)
        .bind(&digests.audit_event_id)
        .bind(&digests.apply_attempt_id)
        .bind(&digests.deployment_id)
        .bind(Json(locked_projection))
        .bind(prepared.deployment.desired_target_digest())
        .bind(Json(previous_runtime))
        .bind(Json(prepared.deployment.snapshot_json()))
        .bind(Json(&prepared.activation_notices))
        .fetch_one(&mut **transaction)
        .await
}
