use authoring_application::{AuthorizedPromotionAccessV1, AuthorizedPromotionSubmissionErrorV1};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use super::admission::{
    product_promotion_admission_context_v1, ProductPromotionAdmissionContextV1,
};
use super::authorization::{product_promotion_access_args_v1, ProductPromotionAccessArgsV1};
use super::digest::{promotion_digests_v1, ProductPromotionDigestsV1};
use super::row::{
    decode_product_promotion_approval_environment_v1,
    validate_product_promotion_admitted_for_access_v1, ProductPromotionAdmittedStageV1,
    ProductPromotionApprovalEnvironmentDecodedV1, ProductPromotionApprovalEnvironmentOutcomeV1,
    ProductPromotionApprovalEnvironmentRowV1, ProductPromotionApprovalEnvironmentStageV1,
};
use super::store::PostgresProductPromotions;
use super::transaction::{
    configure_product_promotion_transaction_v1, map_product_promotion_backend_v1,
    map_product_promotion_commit_v1, map_product_promotion_query_v1, retryable_rollback_v1,
};

const APPROVAL_ENVIRONMENT_SQL: &str =
    "SELECT outcome_code, promotion_record, historical_binding_revision, historical_resource_bindings, \
     historical_binding_fingerprint, active_version, active_content_hash, \
     target_artifact_projection, database_now \
     FROM public.starring_product_promotion_approval_environment_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
     $16, $17, $18, $19) LIMIT 2";

impl PostgresProductPromotions {
    pub(crate) async fn resolve_authorized_promotion_approval_environment_stage_v1(
        &self,
        access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
        admitted: ProductPromotionAdmittedStageV1,
    ) -> Result<ProductPromotionApprovalEnvironmentOutcomeV1, AuthorizedPromotionSubmissionErrorV1>
    {
        let access_args = product_promotion_access_args_v1(access)?;
        let context = product_promotion_admission_context_v1(access);
        let digests = promotion_digests_v1(self.config.keyring(), access)
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)?;
        validate_product_promotion_admitted_for_access_v1(
            &admitted,
            self.config.keyring(),
            &context,
            &access_args,
            &digests,
        )?;
        self.execute_approval_environment_stage_v1(&access_args, &context, &digests, admitted)
            .await
    }

    async fn execute_approval_environment_stage_v1(
        &self,
        access: &ProductPromotionAccessArgsV1,
        context: &ProductPromotionAdmissionContextV1,
        digests: &ProductPromotionDigestsV1,
        admitted: ProductPromotionAdmittedStageV1,
    ) -> Result<ProductPromotionApprovalEnvironmentOutcomeV1, AuthorizedPromotionSubmissionErrorV1>
    {
        let expected_revision = i64::try_from(admitted.record.revision.get())
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
        if admitted.record.id != digests.promotion_id
            || admitted.record.intent.authority.session_id != context.authoring_session_id
            || admitted.record.intent.authority.session_generation != context.generation
        {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
        }
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
            let rows = sqlx::query_as::<_, ProductPromotionApprovalEnvironmentRowV1>(
                APPROVAL_ENVIRONMENT_SQL,
            )
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
            .bind(admitted.record.id.as_str())
            .bind(expected_revision)
            .bind(admitted.record.request_digest.as_str())
            .bind(&admitted.admission_digest)
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
            let decoded = match decode_product_promotion_approval_environment_v1(row, &admitted) {
                Ok(decoded) => decoded,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(error);
                }
            };
            match transaction.commit().await {
                Ok(()) => {
                    return Ok(match decoded {
                        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
                            resolved,
                            target_artifact,
                            database_now,
                        } => ProductPromotionApprovalEnvironmentOutcomeV1::Resolved(Box::new(
                            ProductPromotionApprovalEnvironmentStageV1 {
                                admitted: ProductPromotionAdmittedStageV1 {
                                    record: admitted.record,
                                    admission: admitted.admission,
                                    admission_digest: admitted.admission_digest,
                                    database_now,
                                },
                                resolved,
                                target_artifact: *target_artifact,
                            },
                        )),
                        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired {
                            record,
                            database_now,
                        } => ProductPromotionApprovalEnvironmentOutcomeV1::FinalReplayRequired(
                            Box::new(ProductPromotionAdmittedStageV1 {
                                record: *record,
                                admission: admitted.admission,
                                admission_digest: admitted.admission_digest,
                                database_now,
                            }),
                        ),
                    });
                }
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
    fn approval_environment_query_is_bounded_and_matches_the_exact_argument_count() {
        assert!(APPROVAL_ENVIRONMENT_SQL.ends_with("LIMIT 2"));
        for ordinal in 1..=19 {
            assert!(APPROVAL_ENVIRONMENT_SQL.contains(&format!("${ordinal}")));
        }
        assert!(!APPROVAL_ENVIRONMENT_SQL.contains("$20"));
        for projection in [
            "promotion_record",
            "historical_binding_revision",
            "historical_resource_bindings",
            "historical_binding_fingerprint",
            "active_version",
            "active_content_hash",
            "target_artifact_projection",
        ] {
            assert!(APPROVAL_ENVIRONMENT_SQL.contains(projection));
        }
    }
}
