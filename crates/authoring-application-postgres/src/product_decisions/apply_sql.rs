use authoring_application::{AuthorizedApplyProductV1, FreshGuildAuthorityEvidence};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use super::apply_contract::{
    BEGIN_RUNTIME_DRAIN_QUERY, FINALIZE_QUERY, LOCK_QUERY, TARGET_ARTIFACT_QUERY,
};
use super::apply_projection::PreparedProductApplyV1;
use super::digest::ApplyDigests;
use super::runtime_identity::RuntimeDrainCandidateIdsV2;

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
pub(super) struct ApplyBeginRuntimeDrainRow {
    pub outcome: String,
    pub locked_snapshot: Option<Json<Value>>,
    pub observed_at: Option<DateTime<Utc>>,
    pub product_tenant_id: Option<String>,
    pub product_installation_id: Option<String>,
    pub product_deployment_id: Option<String>,
    pub product_expected_revision: Option<i64>,
    pub product_operation_id: Option<String>,
    pub product_expected_target: Option<Json<Value>>,
    pub product_mutation_request_bytes: Option<Vec<u8>>,
    pub product_mutation_digest: Option<String>,
    pub drain_tenant_id: Option<String>,
    pub drain_installation_id: Option<String>,
    pub drain_deployment_id: Option<String>,
    pub drain_slot_guild_id: Option<String>,
    pub drain_slot_ruleset_key: Option<String>,
    pub drain_expected_revision: Option<i64>,
    pub drain_intent_id: Option<String>,
    pub drain_intent_request_bytes: Option<Vec<u8>>,
    pub drain_intent_digest: Option<String>,
    pub intent_revision: Option<i64>,
    pub intent_state: Option<String>,
    pub canonical_state_bytes: Option<Vec<u8>>,
    pub canonical_state_digest: Option<String>,
    pub writer_epoch_before: Option<i64>,
    pub writer_epoch_after: Option<i64>,
    pub pending_drain_intent_id: Option<String>,
    pub pending_product_operation_id: Option<String>,
    pub pending_tenant_id: Option<String>,
    pub pending_installation_id: Option<String>,
    pub pending_deployment_id: Option<String>,
    pub pending_expected_revision: Option<i64>,
    pub pending_marked_at: Option<DateTime<Utc>>,
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

pub(super) async fn begin_runtime_drain(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    candidates: Option<&RuntimeDrainCandidateIdsV2>,
    expected_revision: i64,
    authority_revision: i64,
) -> Result<ApplyBeginRuntimeDrainRow, sqlx::Error> {
    let evidence = request.evidence();
    sqlx::query_as::<_, ApplyBeginRuntimeDrainRow>(BEGIN_RUNTIME_DRAIN_QUERY)
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
        .bind(
            candidates
                .map(|candidate| candidate.product_operation_id.as_str())
                .unwrap_or(""),
        )
        .bind(
            candidates
                .map(|candidate| candidate.drain_intent_id.as_str())
                .unwrap_or(""),
        )
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
