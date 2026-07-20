use authoring_application::{
    AuthorizedRejectProductV1, CapabilityV1, FreshGuildAuthorityEvidence, ProductControlPortError,
    ProductDecisionPhaseV1, ProductDecisionProjectionV1, ProductMutationReceiptV1,
    ProductRejectionPort,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use sqlx::postgres::PgPool;

use super::config::{PostgresProductDecisionsConfig, ProductDecisionConfigError};
use super::database::{configure_mutation_transaction, database_backend, database_commit};
use super::digest::rejection_digests;
use super::row::{approval_guild_from_database, approval_revision_from_database};
use crate::product_action_digest::ProductActionDigestKeyringV1;

const REJECTION_QUERY: &str =
    "SELECT outcome, resulting_revision, resulting_state, exact_replay, guild_id \
    FROM public.starring_product_reject_v1(\
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
    $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29)";

#[derive(Clone)]
pub struct PostgresProductRejections {
    pub(super) rejection_executor: PgPool,
    pub(super) config: PostgresProductDecisionsConfig,
}

impl PostgresProductRejections {
    pub fn new(
        rejection_executor: PgPool,
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Ok(Self {
            rejection_executor,
            config: PostgresProductDecisionsConfig::production(keyring)?,
        })
    }

    pub fn with_config(rejection_executor: PgPool, config: PostgresProductDecisionsConfig) -> Self {
        Self {
            rejection_executor,
            config,
        }
    }
}

impl ProductRejectionPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductRejections {
    async fn reject_payload_bound(
        &self,
        request: AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        validate_rejection_evidence(&request)?;
        let digests = rejection_digests(self.config.keyring(), &request);
        let evidence = request.evidence();
        let observed_at = evidence.observed_at();
        let expires_at = evidence.expires_at();
        let permission_bits = evidence.effective_permissions_bits().to_string();
        let mut transaction = self
            .rejection_executor
            .begin()
            .await
            .map_err(database_backend)?;
        configure_mutation_transaction(&mut transaction, &self.config).await?;
        let outcome = sqlx::query_as::<_, RejectionOutcomeRow>(REJECTION_QUERY)
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
            .bind("reject")
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
            .bind(request.command().reason.as_str())
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
            return Err(map_rejection_failure(&outcome));
        }
        let projection = match rejection_projection(&request, &outcome) {
            Ok(projection) => projection,
            Err(error) => {
                transaction.rollback().await.map_err(database_backend)?;
                return Err(error);
            }
        };
        transaction.commit().await.map_err(|error| {
            database_commit(error, "product rejection commit outcome is unavailable")
        })?;
        Ok(ProductMutationReceiptV1::from_server_projection(
            projection,
            outcome.exact_replay,
        ))
    }
}

fn validate_rejection_evidence(
    request: &AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), ProductControlPortError> {
    let evidence = request.evidence();
    if evidence.capability() != CapabilityV1::Reject {
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

fn rejection_projection(
    request: &AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    outcome: &RejectionOutcomeRow,
) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
    let revision = approval_revision_from_database(
        outcome
            .resulting_revision
            .ok_or_else(invalid_rejection_result)?,
    )?;
    if request
        .command()
        .expected_revision
        .get()
        .checked_add(1)
        .filter(|expected| *expected == revision.get())
        .is_none()
    {
        return Err(invalid_rejection_result());
    }
    let phase = rejection_phase_from_database(
        outcome
            .resulting_state
            .as_deref()
            .ok_or_else(invalid_rejection_result)?,
    )?;
    let guild_id = approval_guild_from_database(
        outcome
            .guild_id
            .as_deref()
            .ok_or_else(invalid_rejection_result)?,
    )?;
    if guild_id != request.scope().guild_id() {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    Ok(ProductDecisionProjectionV1::from_server_projection(
        request.scope().tenant_id().clone(),
        request.scope().installation_id().clone(),
        guild_id,
        request.command().promotion.promotion_id().clone(),
        revision,
        phase,
    ))
}

#[derive(sqlx::FromRow)]
struct RejectionOutcomeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
}

fn rejection_phase_from_database(
    state: &str,
) -> Result<ProductDecisionPhaseV1, ProductControlPortError> {
    match state {
        "rejected" => Ok(ProductDecisionPhaseV1::Rejected),
        _ => Err(invalid_rejection_result()),
    }
}

fn map_rejection_outcome(outcome: &str) -> ProductControlPortError {
    match outcome {
        "not_found" => ProductControlPortError::NotFound,
        "scope_mismatch" => ProductControlPortError::ScopeMismatch,
        "revision_conflict" => ProductControlPortError::RevisionConflict,
        "payload_mismatch" => ProductControlPortError::PayloadMismatch,
        "invalid_state" | "authorization_stale" | "authority_mismatch" => {
            ProductControlPortError::InvalidState
        }
        "expired" => ProductControlPortError::Expired,
        "idempotency_conflict" => ProductControlPortError::IdempotencyConflict,
        "idempotency_keyring_incomplete" => ProductControlPortError::Backend(
            "product rejection idempotency keyring does not cover live receipts".to_string(),
        ),
        "indeterminate" => ProductControlPortError::Indeterminate(
            "persisted product rejection receipt is incomplete".to_string(),
        ),
        _ => invalid_rejection_result(),
    }
}

fn map_rejection_failure(outcome: &RejectionOutcomeRow) -> ProductControlPortError {
    if outcome.resulting_revision.is_some()
        || outcome.resulting_state.is_some()
        || outcome.exact_replay
        || outcome.guild_id.is_some()
    {
        return invalid_rejection_result();
    }
    map_rejection_outcome(&outcome.outcome)
}

fn invalid_rejection_result() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "product rejection function returned an invalid result".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_result_accepts_only_rejected_phase() {
        assert_eq!(
            rejection_phase_from_database("rejected").unwrap(),
            ProductDecisionPhaseV1::Rejected
        );
        for state in ["pending", "approved", "expired", "applied", ""] {
            assert_eq!(
                rejection_phase_from_database(state).unwrap_err(),
                invalid_rejection_result()
            );
        }
    }

    #[test]
    fn rejection_outcome_mapping_is_closed() {
        for (outcome, expected) in [
            ("not_found", ProductControlPortError::NotFound),
            ("scope_mismatch", ProductControlPortError::ScopeMismatch),
            (
                "revision_conflict",
                ProductControlPortError::RevisionConflict,
            ),
            ("payload_mismatch", ProductControlPortError::PayloadMismatch),
            ("invalid_state", ProductControlPortError::InvalidState),
            ("authorization_stale", ProductControlPortError::InvalidState),
            ("authority_mismatch", ProductControlPortError::InvalidState),
            ("expired", ProductControlPortError::Expired),
            (
                "idempotency_conflict",
                ProductControlPortError::IdempotencyConflict,
            ),
        ] {
            assert_eq!(map_rejection_outcome(outcome), expected);
        }
        assert!(matches!(
            map_rejection_outcome("idempotency_keyring_incomplete"),
            ProductControlPortError::Backend(_)
        ));
        assert!(matches!(
            map_rejection_outcome("indeterminate"),
            ProductControlPortError::Indeterminate(_)
        ));
        for outcome in [
            "ok",
            "invalid_input",
            "duplicate_decision",
            "self_approval_forbidden",
            "unexpected",
            "",
        ] {
            assert_eq!(map_rejection_outcome(outcome), invalid_rejection_result());
        }
    }

    #[test]
    fn rejection_error_rows_cannot_carry_success_projection() {
        let clean = RejectionOutcomeRow {
            outcome: "expired".to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
        };
        assert_eq!(
            map_rejection_failure(&clean),
            ProductControlPortError::Expired
        );
        for contaminated in [
            RejectionOutcomeRow {
                resulting_revision: Some(4),
                ..clean_row("expired")
            },
            RejectionOutcomeRow {
                resulting_state: Some("rejected".to_string()),
                ..clean_row("expired")
            },
            RejectionOutcomeRow {
                exact_replay: true,
                ..clean_row("expired")
            },
            RejectionOutcomeRow {
                guild_id: Some("42".to_string()),
                ..clean_row("expired")
            },
        ] {
            assert_eq!(
                map_rejection_failure(&contaminated),
                invalid_rejection_result()
            );
        }
    }

    #[test]
    fn rejection_query_selects_exact_projection_and_binds_reason_last() {
        assert!(REJECTION_QUERY.starts_with(
            "SELECT outcome, resulting_revision, resulting_state, exact_replay, guild_id"
        ));
        assert!(REJECTION_QUERY.contains("public.starring_product_reject_v1("));
        assert!(REJECTION_QUERY.ends_with("$28, $29)"));
        assert_eq!(REJECTION_QUERY.matches('$').count(), 29);
    }

    fn clean_row(outcome: &str) -> RejectionOutcomeRow {
        RejectionOutcomeRow {
            outcome: outcome.to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
        }
    }
}
