use authoring_application::{AuthorizedPromotionAccessV1, AuthorizedPromotionSubmissionErrorV1};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;

use super::admission::{
    prepare_legacy_product_promotion_admission_v1, product_promotion_admission_context_v1,
    PreparedProductPromotionAdmissionV1, ProductPromotionAdmissionContextV1,
    ProductPromotionAdmissionErrorV1,
};
use super::authorization::{product_promotion_access_args_v1, ProductPromotionAccessArgsV1};
use super::digest::{promotion_digests_v1, ProductPromotionDigestsV1};
use super::row::{
    decode_product_promotion_repair_link_v1, validate_product_promotion_legacy_for_access_v1,
    ProductPromotionActivationLinkRowV1, ProductPromotionLegacyRepairStageV1,
    ProductPromotionLegacyRepairV1,
};
use super::store::PostgresProductPromotions;
use super::transaction::{
    configure_product_promotion_transaction_v1, map_product_promotion_backend_v1,
    map_product_promotion_commit_v1, map_product_promotion_query_v1, retryable_rollback_v1,
};

const MAX_RECOVERY_ADMISSION_PAYLOAD_BYTES: usize = 32_768;
const REPAIR_LINK_SQL: &str =
    "SELECT outcome_code, promotion_record, admission_evidence, admission_digest, \
     activation_projection, receipt_projection, audit_evidence_projection, database_now \
     FROM public.starring_product_promotion_repair_link_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
     $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29) LIMIT 2";

impl PostgresProductPromotions {
    pub(crate) async fn repair_legacy_authorized_promotion_stage_v1(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
        legacy: ProductPromotionLegacyRepairV1,
    ) -> Result<ProductPromotionLegacyRepairStageV1, AuthorizedPromotionSubmissionErrorV1> {
        let access_args = product_promotion_access_args_v1(access)?;
        let context = product_promotion_admission_context_v1(access);
        let digests = promotion_digests_v1(self.config.keyring(), access)
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        validate_product_promotion_legacy_for_access_v1(&legacy, &context, &access_args, &digests)?;
        let admission = prepare_legacy_product_promotion_admission_v1(
            self.config.keyring(),
            &context,
            &access_args,
            &legacy.record,
            &digests,
        )
        .map_err(map_recovery_admission_error_v1)?;
        let serialized = SerializedProductPromotionRepairV1::new(&admission)?;
        self.execute_legacy_repair_stage_v1(
            &access_args,
            &context,
            &digests,
            legacy,
            &admission,
            &serialized,
        )
        .await
    }

    async fn execute_legacy_repair_stage_v1(
        &self,
        access: &ProductPromotionAccessArgsV1,
        context: &ProductPromotionAdmissionContextV1,
        digests: &ProductPromotionDigestsV1,
        legacy: ProductPromotionLegacyRepairV1,
        admission: &PreparedProductPromotionAdmissionV1,
        serialized: &SerializedProductPromotionRepairV1,
    ) -> Result<ProductPromotionLegacyRepairStageV1, AuthorizedPromotionSubmissionErrorV1> {
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
            let rows = sqlx::query_as::<_, ProductPromotionActivationLinkRowV1>(REPAIR_LINK_SQL)
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
                .bind(legacy.record.id.as_str())
                .bind(legacy.record.request_digest.as_str())
                .bind(&context.product_request_id)
                .bind(&digests.session_subject)
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
            let decoded = match decode_product_promotion_repair_link_v1(
                row,
                self.config.keyring(),
                context,
                access,
                digests,
                &legacy,
                admission,
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

fn map_recovery_admission_error_v1(
    _: ProductPromotionAdmissionErrorV1,
) -> AuthorizedPromotionSubmissionErrorV1 {
    AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
}

struct SerializedProductPromotionRepairV1 {
    admission_payload: Json<Value>,
}

impl SerializedProductPromotionRepairV1 {
    fn new(
        admission: &PreparedProductPromotionAdmissionV1,
    ) -> Result<Self, AuthorizedPromotionSubmissionErrorV1> {
        Ok(Self {
            admission_payload: bounded_json_value_v1(
                &admission.payload,
                MAX_RECOVERY_ADMISSION_PAYLOAD_BYTES,
            )?,
        })
    }
}

fn bounded_json_value_v1(
    value: &impl Serialize,
    maximum: usize,
) -> Result<Json<Value>, AuthorizedPromotionSubmissionErrorV1> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if bytes.len() > maximum {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn repair_link_query_is_bounded_and_matches_the_exact_contract() {
        assert!(REPAIR_LINK_SQL.ends_with("LIMIT 2"));
        for ordinal in 1..=29 {
            assert!(REPAIR_LINK_SQL.contains(&format!("${ordinal}")));
        }
        assert!(!REPAIR_LINK_SQL.contains("$30"));
        assert!(REPAIR_LINK_SQL.contains("starring_product_promotion_repair_link_v1"));
        for projection in [
            "promotion_record",
            "admission_evidence",
            "admission_digest",
            "activation_projection",
            "receipt_projection",
            "audit_evidence_projection",
        ] {
            assert!(REPAIR_LINK_SQL.contains(projection));
        }
    }

    #[test]
    fn recovery_admission_serialization_is_bounded() {
        let oversized = json!({"padding": "x".repeat(MAX_RECOVERY_ADMISSION_PAYLOAD_BYTES + 1)});
        assert!(matches!(
            bounded_json_value_v1(&oversized, MAX_RECOVERY_ADMISSION_PAYLOAD_BYTES),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn durable_recovery_admission_failures_are_persistence_corruption() {
        for error in [
            ProductPromotionAdmissionErrorV1::ProjectionMismatch,
            ProductPromotionAdmissionErrorV1::ScalarOverflow,
            ProductPromotionAdmissionErrorV1::Serialization,
            ProductPromotionAdmissionErrorV1::PayloadTooLarge,
            ProductPromotionAdmissionErrorV1::InvalidFormat,
            ProductPromotionAdmissionErrorV1::KeyUnavailable,
            ProductPromotionAdmissionErrorV1::DigestMismatch,
        ] {
            assert_eq!(
                map_recovery_admission_error_v1(error),
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
            );
        }
    }
}
