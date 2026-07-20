use authoring_application::{
    AuthorizedPromotionSubmissionErrorV1, AuthorizedPromotionSubmissionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use authoring_promotion::{plan_start_promotion_v1, PreparedPromotionPlanV1};
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;

use super::admission::{
    prepare_product_promotion_admission_v1, product_promotion_admission_context_v1,
    PreparedProductPromotionAdmissionV1, ProductPromotionAdmissionContextV1,
};
use super::authorization::{
    validate_product_promotion_submission_v1, ProductPromotionAccessArgsV1,
};
use super::digest::{promotion_digests_v1, ProductPromotionDigestsV1};
use super::row::{
    decode_product_promotion_prepare_v1, ProductPromotionPrepareRowV1,
    ProductPromotionPrepareStageV1, ProductPromotionReplayStageV1,
};
use super::store::PostgresProductPromotions;
use super::transaction::{
    configure_product_promotion_transaction_v1, map_product_promotion_backend_v1,
    map_product_promotion_commit_v1, map_product_promotion_query_v1, retryable_rollback_v1,
};

const MAX_PREPARED_INTENT_BYTES: usize = 8_388_608;
const MAX_RULESET_DEFINITION_BYTES: usize = 524_288;
const MAX_ADMISSION_PAYLOAD_BYTES: usize = 32_768;
const PREPARE_SQL: &str =
    "SELECT outcome_code, promotion_record, admission_evidence, admission_digest, database_now \
     FROM public.starring_product_promotion_prepare_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
     $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, \
     $31, $32, $33, $34, $35) LIMIT 2";

impl PostgresProductPromotions {
    pub(crate) async fn prepare_authorized_promotion_stage_v1(
        &self,
        request: AuthorizedPromotionSubmissionV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductPromotionPrepareStageV1, AuthorizedPromotionSubmissionErrorV1> {
        let access_args = validate_product_promotion_submission_v1(&request)?;
        let context = product_promotion_admission_context_v1(request.access());
        let digests = promotion_digests_v1(self.config.keyring(), request.access())
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        let plan = plan_start_promotion_v1(request.into_input())
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        let admission = prepare_product_promotion_admission_v1(
            self.config.keyring(),
            &context,
            &access_args,
            &plan,
            &digests,
        )
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        let serialized = SerializedProductPromotionPrepareV1::new(&plan, &admission)?;
        let stage = self
            .execute_prepare_stage_v1(
                &access_args,
                &context,
                &digests,
                &plan,
                &admission,
                &serialized,
            )
            .await?;
        match stage {
            ProductPromotionPrepareStageV1::FinalReplayRequired(_) => {
                match self
                    .execute_replay_stage_v1(&access_args, &context, &digests)
                    .await?
                {
                    ProductPromotionReplayStageV1::FinalExact(final_replay) => {
                        Ok(ProductPromotionPrepareStageV1::FinalExact(final_replay))
                    }
                    ProductPromotionReplayStageV1::Missing
                    | ProductPromotionReplayStageV1::PartialExact(_)
                    | ProductPromotionReplayStageV1::LegacyRepairRequired(_) => {
                        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
                    }
                }
            }
            stage => Ok(stage),
        }
    }

    async fn execute_prepare_stage_v1(
        &self,
        access: &ProductPromotionAccessArgsV1,
        context: &ProductPromotionAdmissionContextV1,
        digests: &ProductPromotionDigestsV1,
        plan: &PreparedPromotionPlanV1,
        admission: &PreparedProductPromotionAdmissionV1,
        serialized: &SerializedProductPromotionPrepareV1,
    ) -> Result<ProductPromotionPrepareStageV1, AuthorizedPromotionSubmissionErrorV1> {
        let expected_generation = positive_i64_v1(context.generation.get())?;
        let expected_candidate_revision = positive_i64_v1(plan.intent.evidence.candidate_revision)?;
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
            let rows = sqlx::query_as::<_, ProductPromotionPrepareRowV1>(PREPARE_SQL)
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
                .bind(&context.product_request_id)
                .bind(&digests.session_subject)
                .bind(context.authoring_session_id.as_str())
                .bind(expected_generation)
                .bind(expected_candidate_revision)
                .bind(plan.intent.evidence.candidate_ruleset_hash.as_str())
                .bind(plan.intent.evidence.context_fingerprint.as_str())
                .bind(plan.promotion_id.as_str())
                .bind(plan.request_digest.as_str())
                .bind(&serialized.intent)
                .bind(&serialized.admission_payload)
                .bind(&admission.digest)
                .bind(&digests.active_idempotency)
                .bind(&digests.idempotency_candidates)
                .bind(&digests.idempotency_candidate_key_ids)
                .bind(&digests.idempotency_candidate_key_fingerprints)
                .bind(&digests.active_key_id)
                .bind(&digests.semantic_request)
                .bind(&digests.receipt_id)
                .bind(&digests.audit_event_id)
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
            let result = decode_product_promotion_prepare_v1(
                row,
                self.config.keyring(),
                context,
                access,
                digests,
                plan,
                admission,
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

struct SerializedProductPromotionPrepareV1 {
    intent: Json<Value>,
    admission_payload: Json<Value>,
}

impl SerializedProductPromotionPrepareV1 {
    fn new(
        plan: &PreparedPromotionPlanV1,
        admission: &PreparedProductPromotionAdmissionV1,
    ) -> Result<Self, AuthorizedPromotionSubmissionErrorV1> {
        bounded_json_value_v1(&plan.intent.definition, MAX_RULESET_DEFINITION_BYTES)?;
        let intent = bounded_json_value_v1(&plan.intent, MAX_PREPARED_INTENT_BYTES)?;
        let admission_payload =
            bounded_json_value_v1(&admission.payload, MAX_ADMISSION_PAYLOAD_BYTES)?;
        Ok(Self {
            intent,
            admission_payload,
        })
    }
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

fn positive_i64_v1(value: u64) -> Result<i64, AuthorizedPromotionSubmissionErrorV1> {
    if value == 0 {
        return Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate);
    }
    i64::try_from(value).map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn prepare_query_is_bounded_and_matches_the_exact_argument_count() {
        assert!(PREPARE_SQL.ends_with("LIMIT 2"));
        for ordinal in 1..=35 {
            assert!(PREPARE_SQL.contains(&format!("${ordinal}")));
        }
        assert!(!PREPARE_SQL.contains("$36"));
        assert!(PREPARE_SQL.contains("admission_digest"));
    }

    #[test]
    fn serialized_inputs_are_bounded_before_database_submission() {
        assert!(bounded_json_value_v1(&json!({"ok": true}), 16).is_ok());
        assert_eq!(
            bounded_json_value_v1(&json!({"value": "x".repeat(64)}), 16).unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::InvalidCandidate
        );
    }

    #[test]
    fn positive_database_scalars_reject_zero_and_overflow() {
        assert_eq!(positive_i64_v1(1), Ok(1));
        assert_eq!(
            positive_i64_v1(0),
            Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)
        );
        assert_eq!(
            positive_i64_v1(i64::MAX as u64 + 1),
            Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)
        );
    }
}

#[cfg(test)]
pub(super) mod postgres_tests;
