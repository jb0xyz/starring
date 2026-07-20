use authoring_application::{AuthorizedPromotionAccessV1, AuthorizedPromotionSubmissionErrorV1};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use authoring_promotion::{plan_pending_activation_v1, PendingActivationProposalV1};
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;

use super::admission::{
    product_promotion_admission_context_v1, ProductPromotionAdmissionContextV1,
};
use super::authorization::{product_promotion_access_args_v1, ProductPromotionAccessArgsV1};
use super::digest::{promotion_digests_v1, ProductPromotionDigestsV1};
use super::row::{
    decode_product_promotion_activation_link_v1, validate_product_promotion_admitted_for_access_v1,
    ProductPromotionActivationLinkRowV1, ProductPromotionApprovalEnvironmentStageV1,
    ProductPromotionFinalReplayV1,
};
use super::store::PostgresProductPromotions;
use super::transaction::{
    configure_product_promotion_transaction_v1, map_product_promotion_backend_v1,
    map_product_promotion_commit_v1, map_product_promotion_query_v1, retryable_rollback_v1,
};

const MAX_ACTIVATION_PROPOSAL_BYTES: usize = 1_048_576;
const ACTIVATION_PROPOSAL_FORMAT_VERSION: u16 = 1;
const ACTIVATION_LINK_SQL: &str =
    "SELECT outcome_code, promotion_record, admission_evidence, admission_digest, \
     activation_projection, receipt_projection, audit_evidence_projection, database_now \
     FROM public.starring_product_promotion_activation_link_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
     $16, $17, $18, $19, $20) LIMIT 2";

impl PostgresProductPromotions {
    pub(crate) async fn link_authorized_promotion_activation_stage_v1(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
        environment: ProductPromotionApprovalEnvironmentStageV1,
    ) -> Result<ProductPromotionFinalReplayV1, AuthorizedPromotionSubmissionErrorV1> {
        let access_args = product_promotion_access_args_v1(access)?;
        let context = product_promotion_admission_context_v1(access);
        let digests = promotion_digests_v1(self.config.keyring(), access)
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        validate_product_promotion_admitted_for_access_v1(
            &environment.admitted,
            self.config.keyring(),
            &context,
            &access_args,
            &digests,
        )?;
        let proposal =
            plan_pending_activation_v1(&environment.admitted.record, environment.resolved.clone())
                .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
        let serialized = bounded_json_value_v1(
            &ProductPromotionActivationProposalEnvelopeV1 {
                format_version: ACTIVATION_PROPOSAL_FORMAT_VERSION,
                proposal: &proposal,
            },
            MAX_ACTIVATION_PROPOSAL_BYTES,
        )?;
        self.execute_activation_link_stage_v1(
            &access_args,
            &context,
            &digests,
            environment,
            &proposal,
            &serialized,
        )
        .await
    }

    async fn execute_activation_link_stage_v1(
        &self,
        access: &ProductPromotionAccessArgsV1,
        context: &ProductPromotionAdmissionContextV1,
        digests: &ProductPromotionDigestsV1,
        environment: ProductPromotionApprovalEnvironmentStageV1,
        proposal: &PendingActivationProposalV1,
        serialized: &Json<Value>,
    ) -> Result<ProductPromotionFinalReplayV1, AuthorizedPromotionSubmissionErrorV1> {
        let expected_revision = i64::try_from(environment.admitted.record.revision.get())
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
        let mut retries = 0_u8;
        loop {
            let mut transaction = match self.executor.begin().await {
                Ok(transaction) => transaction,
                Err(error) => {
                    if retryable_rollback_v1(&error)
                        && retries < self.config.transaction_retry_limit()
                    {
                        retries += 1;
                        continue;
                    }
                    return Err(map_product_promotion_backend_v1(&error));
                }
            };
            if let Err(error) =
                configure_product_promotion_transaction_v1(&mut transaction, &self.config).await
            {
                let retry = retryable_rollback_v1(&error)
                    && retries < self.config.transaction_retry_limit();
                let _ = transaction.rollback().await;
                if retry {
                    retries += 1;
                    continue;
                }
                return Err(map_product_promotion_backend_v1(&error));
            }
            let rows =
                sqlx::query_as::<_, ProductPromotionActivationLinkRowV1>(ACTIVATION_LINK_SQL)
                    .bind(&access.expected_tenant_id)
                    .bind(&access.expected_installation_id)
                    .bind(&access.expected_principal_id)
                    .bind(&access.expected_product_session_digest)
                    .bind(&access.expected_acting_user_id)
                    .bind(&access.expected_discord_application_id)
                    .bind(&access.expected_guild_id)
                    .bind(&access.expected_capability)
                    .bind(access.observed_current_authority_revision)
                    .bind(&access.observed_current_authority_payload_digest)
                    .bind(&access.authority_observation_digest)
                    .bind(access.authority_observed_at)
                    .bind(access.authority_expires_at)
                    .bind(&access.effective_permission_bits)
                    .bind(access.guild_owner)
                    .bind(environment.admitted.record.id.as_str())
                    .bind(expected_revision)
                    .bind(environment.admitted.record.request_digest.as_str())
                    .bind(&environment.admitted.admission_digest)
                    .bind(serialized)
                    .fetch_all(&mut *transaction)
                    .await;
            let rows = match rows {
                Ok(rows) => rows,
                Err(error) => {
                    let retry = retryable_rollback_v1(&error)
                        && retries < self.config.transaction_retry_limit();
                    let _ = transaction.rollback().await;
                    if retry {
                        retries += 1;
                        continue;
                    }
                    return Err(map_product_promotion_query_v1(&error));
                }
            };
            if rows.len() != 1 {
                let _ = transaction.rollback().await;
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            let row = rows
                .into_iter()
                .next()
                .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            let decoded = match decode_product_promotion_activation_link_v1(
                row,
                self.config.keyring(),
                context,
                access,
                digests,
                &environment,
                proposal,
            ) {
                Ok(decoded) => decoded,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(error);
                }
            };
            match transaction.commit().await {
                Ok(()) => return Ok(decoded),
                Err(error) => {
                    if retryable_rollback_v1(&error)
                        && retries < self.config.transaction_retry_limit()
                    {
                        retries += 1;
                        continue;
                    }
                    return Err(map_product_promotion_commit_v1(&error));
                }
            }
        }
    }
}

#[derive(Serialize)]
struct ProductPromotionActivationProposalEnvelopeV1<'a, T: ?Sized> {
    format_version: u16,
    proposal: &'a T,
}

fn bounded_json_value_v1(
    value: &impl Serialize,
    maximum: usize,
) -> Result<Json<Value>, AuthorizedPromotionSubmissionErrorV1> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
    if bytes.len() > maximum {
        return Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate);
    }
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn activation_link_query_is_bounded_and_matches_the_exact_argument_count() {
        assert!(ACTIVATION_LINK_SQL.ends_with("LIMIT 2"));
        for ordinal in 1..=20 {
            assert!(ACTIVATION_LINK_SQL.contains(&format!("${ordinal}")));
        }
        assert!(!ACTIVATION_LINK_SQL.contains("$21"));
        for projection in [
            "promotion_record",
            "admission_evidence",
            "admission_digest",
            "activation_projection",
            "receipt_projection",
            "audit_evidence_projection",
        ] {
            assert!(ACTIVATION_LINK_SQL.contains(projection));
        }
    }

    #[test]
    fn activation_proposal_serialization_is_bounded() {
        let oversized = json!({"padding": "x".repeat(MAX_ACTIVATION_PROPOSAL_BYTES + 1)});
        assert!(matches!(
            bounded_json_value_v1(&oversized, MAX_ACTIVATION_PROPOSAL_BYTES),
            Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)
        ));
    }

    #[test]
    fn activation_proposal_envelope_has_an_exact_versioned_shape() {
        let value = serde_json::to_value(ProductPromotionActivationProposalEnvelopeV1 {
            format_version: ACTIVATION_PROPOSAL_FORMAT_VERSION,
            proposal: &json!({"value": "proposal"}),
        })
        .unwrap();
        let fields = value.as_object().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields.get("format_version"), Some(&json!(1)));
        assert_eq!(fields.get("proposal"), Some(&json!({"value": "proposal"})));
    }
}
