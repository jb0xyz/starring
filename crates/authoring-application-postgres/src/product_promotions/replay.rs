use authoring_application::{AuthorizedPromotionAccessV1, AuthorizedPromotionSubmissionErrorV1};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use super::admission::{
    product_promotion_admission_context_v1, ProductPromotionAdmissionContextV1,
};
use super::authorization::{product_promotion_access_args_v1, ProductPromotionAccessArgsV1};
use super::digest::{promotion_digests_v1, ProductPromotionDigestsV1};
use super::row::{
    decode_product_promotion_replay_v1, ProductPromotionReplayRowV1, ProductPromotionReplayStageV1,
};
use super::store::PostgresProductPromotions;
use super::transaction::{
    configure_product_promotion_transaction_v1, map_product_promotion_backend_v1,
    map_product_promotion_commit_v1, map_product_promotion_query_v1, retryable_rollback_v1,
};

const REPLAY_SQL: &str =
    "SELECT outcome_code, promotion_record, admission_evidence, admission_digest, \
     receipt_projection, audit_evidence_projection, database_now \
     FROM public.starring_product_promotion_replay_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
     $16, $17, $18, $19, $20, $21, $22) LIMIT 2";

impl PostgresProductPromotions {
    pub(crate) async fn replay_authorized_promotion_stage_v1(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductPromotionReplayStageV1, AuthorizedPromotionSubmissionErrorV1> {
        let access_args = product_promotion_access_args_v1(access)?;
        let context = product_promotion_admission_context_v1(access);
        let digests = promotion_digests_v1(self.config.keyring(), access)
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        self.execute_replay_stage_v1(&access_args, &context, &digests)
            .await
    }

    pub(super) async fn execute_replay_stage_v1(
        &self,
        access: &ProductPromotionAccessArgsV1,
        context: &ProductPromotionAdmissionContextV1,
        digests: &ProductPromotionDigestsV1,
    ) -> Result<ProductPromotionReplayStageV1, AuthorizedPromotionSubmissionErrorV1> {
        let expected_generation = i64::try_from(context.generation.get())
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
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
            let rows = sqlx::query_as::<_, ProductPromotionReplayRowV1>(REPLAY_SQL)
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
                .bind(digests.promotion_id.as_str())
                .bind(context.authoring_session_id.as_str())
                .bind(expected_generation)
                .bind(&digests.semantic_request)
                .bind(&digests.idempotency_candidates)
                .bind(&digests.idempotency_candidate_key_ids)
                .bind(&digests.idempotency_candidate_key_fingerprints)
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
            let result = decode_product_promotion_replay_v1(
                row,
                self.config.keyring(),
                context,
                access,
                digests,
            );
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(error);
                }
            };
            match transaction.commit().await {
                Ok(()) => return Ok(result),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_query_is_bounded_and_matches_the_exact_argument_count() {
        assert!(REPLAY_SQL.ends_with("LIMIT 2"));
        for ordinal in 1..=22 {
            assert!(REPLAY_SQL.contains(&format!("${ordinal}")));
        }
        assert!(!REPLAY_SQL.contains("$23"));
        assert!(REPLAY_SQL.contains("admission_digest"));
    }
}
