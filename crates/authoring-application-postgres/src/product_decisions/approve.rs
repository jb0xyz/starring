use authoring_application::{
    AuthorizedApproveProductV1, CapabilityV1, FreshGuildAuthorityEvidence, ProductApprovalPort,
    ProductControlPortError, ProductDecisionProjectionV1, ProductMutationReceiptV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use super::database::{configure_mutation_transaction, database_backend, database_commit};
use super::digest::approval_digests;
use super::row::{
    approval_guild_from_database, approval_phase_from_database, approval_revision_from_database,
};
use super::store::PostgresProductDecisions;

impl ProductApprovalPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDecisions {
    async fn approve_payload_bound(
        &self,
        request: AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        validate_approval_evidence(&request)?;
        let digests = approval_digests(self.config.keyring(), &request);
        let evidence = request.evidence();
        let observed_at = evidence.observed_at();
        let expires_at = evidence.expires_at();
        let permission_bits = evidence.effective_permissions_bits().to_string();
        let mut transaction = self
            .pools
            .approval_executor
            .begin()
            .await
            .map_err(database_backend)?;
        configure_mutation_transaction(&mut transaction, &self.config).await?;
        let outcome = sqlx::query_as::<_, ApprovalOutcomeRow>(
            "SELECT outcome, resulting_revision, resulting_state, exact_replay, guild_id \
             FROM public.starring_product_approve_v1(\
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
             $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)",
        )
        .bind(request.scope().tenant_id().as_str())
        .bind(request.scope().installation_id().as_str())
        .bind(request.command().promotion.promotion_id().as_str())
        .bind(
            i64::try_from(request.command().expected_revision.get()).map_err(|_| {
                ProductControlPortError::Backend(
                    "product revision exceeds PostgreSQL range".to_string(),
                )
            })?,
        )
        .bind(request.command().expected_payload_digest.as_str())
        .bind(request.actor().principal_id().as_str())
        .bind(request.session_fingerprint().as_bytes().as_slice())
        .bind(&digests.session_subject)
        .bind(evidence.acting_user_id().to_string())
        .bind(evidence.discord_application_id().get().to_string())
        .bind(evidence.guild_id().to_string())
        .bind("approve")
        .bind(
            i64::try_from(evidence.installation_authority_revision().get()).map_err(|_| {
                ProductControlPortError::Backend(
                    "authority revision exceeds PostgreSQL range".to_string(),
                )
            })?,
        )
        .bind(evidence.installation_authority_digest())
        .bind(evidence.observation_digest())
        .bind(observed_at)
        .bind(expires_at)
        .bind(permission_bits)
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
        .fetch_one(&mut *transaction)
        .await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                transaction.rollback().await.map_err(database_backend)?;
                return Err(database_backend(error));
            }
        };
        if outcome.outcome != "ok" {
            transaction.rollback().await.map_err(database_backend)?;
            return Err(map_approval_outcome(&outcome.outcome));
        }
        let revision = approval_revision_from_database(
            outcome
                .resulting_revision
                .ok_or_else(invalid_approval_result)?,
        )?;
        let phase = approval_phase_from_database(
            outcome
                .resulting_state
                .as_deref()
                .ok_or_else(invalid_approval_result)?,
        )?;
        let guild_id = approval_guild_from_database(
            outcome
                .guild_id
                .as_deref()
                .ok_or_else(invalid_approval_result)?,
        )?;
        if guild_id != request.scope().guild_id() {
            transaction.rollback().await.map_err(database_backend)?;
            return Err(ProductControlPortError::ScopeMismatch);
        }
        let projection = ProductDecisionProjectionV1::from_server_projection(
            request.scope().tenant_id().clone(),
            request.scope().installation_id().clone(),
            guild_id,
            request.command().promotion.promotion_id().clone(),
            revision,
            phase,
        );
        transaction.commit().await.map_err(|error| {
            database_commit(error, "product approval commit outcome is unavailable")
        })?;
        Ok(ProductMutationReceiptV1::from_server_projection(
            projection,
            outcome.exact_replay,
        ))
    }
}

fn validate_approval_evidence(
    request: &AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), ProductControlPortError> {
    let evidence = request.evidence();
    if evidence.capability() != CapabilityV1::Approve {
        return Err(ProductControlPortError::InvalidState);
    }
    if evidence.tenant_id() != request.scope().tenant_id()
        || evidence.installation_id() != request.scope().installation_id()
        || evidence.guild_id() != request.scope().guild_id()
        || evidence.acting_user_id() != request.scope().acting_user_id()
    {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ApprovalOutcomeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
}

fn map_approval_outcome(outcome: &str) -> ProductControlPortError {
    match outcome {
        "not_found" => ProductControlPortError::NotFound,
        "scope_mismatch" => ProductControlPortError::ScopeMismatch,
        "revision_conflict" => ProductControlPortError::RevisionConflict,
        "payload_mismatch" => ProductControlPortError::PayloadMismatch,
        "invalid_state" | "authorization_stale" | "authority_mismatch" => {
            ProductControlPortError::InvalidState
        }
        "self_approval_forbidden" => ProductControlPortError::SelfApprovalForbidden,
        "duplicate_decision" => ProductControlPortError::DuplicateDecision,
        "expired" => ProductControlPortError::Expired,
        "idempotency_conflict" => ProductControlPortError::IdempotencyConflict,
        "idempotency_keyring_incomplete" => ProductControlPortError::Backend(
            "product approval idempotency keyring does not cover live receipts".to_string(),
        ),
        "indeterminate" => ProductControlPortError::Indeterminate(
            "persisted product approval receipt is incomplete".to_string(),
        ),
        _ => invalid_approval_result(),
    }
}

fn invalid_approval_result() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "product approval function returned an invalid result".to_string(),
    )
}
