use authoring_application::{AuthorizedApplyProductV1, FreshGuildAuthorityEvidence};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

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

const LOCK_QUERY: &str = "SELECT outcome, exact_replay, requires_commit, resulting_revision, \
    resulting_state, deployment_id, desired_target_digest, locked_projection \
    FROM public.starring_product_apply_lock_v1(\
        expected_tenant_id => $1, expected_installation_id => $2, \
        expected_promotion_id => $3, expected_product_revision => $4, \
        expected_payload_digest => $5, expected_principal_id => $6, \
        expected_product_session_digest => $7, session_subject_digest => $8, \
        expected_acting_user_id => $9, expected_discord_application_id => $10, \
        expected_guild_id => $11, expected_capability => $12, \
        expected_authority_revision => $13, expected_authority_payload_digest => $14, \
        expected_authority_observation_digest => $15, expected_authority_observed_at => $16, \
        expected_authority_expires_at => $17, expected_effective_permission_bits => $18, \
        expected_guild_owner => $19, product_request_id => $20, \
        active_idempotency_key_digest => $21, idempotency_key_digest_candidates => $22, \
        idempotency_digest_key_id_candidates => $23, \
        idempotency_digest_key_fingerprint_candidates => $24, \
        idempotency_digest_key_id => $25, semantic_request_digest => $26, \
        new_receipt_id => $27, new_audit_event_id => $28, \
        new_apply_attempt_id => $29, new_deployment_id => $30)";

const FINALIZE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, exact_replay, \
    guild_id, deployment_id, desired_target_digest \
    FROM public.starring_product_apply_finalize_v1(\
        expected_tenant_id => $1, expected_installation_id => $2, \
        expected_promotion_id => $3, expected_product_revision => $4, \
        expected_payload_digest => $5, expected_principal_id => $6, \
        expected_product_session_digest => $7, session_subject_digest => $8, \
        expected_acting_user_id => $9, expected_discord_application_id => $10, \
        expected_guild_id => $11, expected_capability => $12, \
        expected_authority_revision => $13, expected_authority_payload_digest => $14, \
        expected_authority_observation_digest => $15, expected_authority_observed_at => $16, \
        expected_authority_expires_at => $17, expected_effective_permission_bits => $18, \
        expected_guild_owner => $19, product_request_id => $20, \
        active_idempotency_key_digest => $21, idempotency_key_digest_candidates => $22, \
        idempotency_digest_key_id_candidates => $23, \
        idempotency_digest_key_fingerprint_candidates => $24, \
        idempotency_digest_key_id => $25, semantic_request_digest => $26, \
        new_receipt_id => $27, new_audit_event_id => $28, \
        new_apply_attempt_id => $29, new_deployment_id => $30, locked_projection => $31, \
        prepared_desired_target_digest => $32, prepared_previous_runtime => $33, \
        prepared_snapshot => $34, prepared_activation_notices => $35)";
