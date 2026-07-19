use authoring_application::{
    AuthorizedApplyProductV1, CapabilityV1, ExactDeploymentSelectorV1, ProductApplyPort,
    ProductControlPortError, ProductDecisionPhaseV1, ProductDecisionProjectionV1,
    ProductMutationReceiptV1, ProductRevisionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_state::InteractionRuleSet;

use super::apply_projection::prepare_product_apply_v1;
use super::apply_sql::{
    finalize_apply, load_apply_target_artifact, lock_apply, ApplyFinalizeRow, ApplyLockRow,
    ApplyTargetArtifactRow,
};
use super::database::{
    commit_failure_proves_rollback, configure_apply_transaction, database_backend, database_commit,
    is_safe_transaction_retry,
};
use super::digest::{apply_digests, ApplyDigests};
use super::store::PostgresProductDecisions;

const MAX_TRANSACTION_ATTEMPTS: usize = 2;

impl ProductApplyPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDecisions {
    async fn apply_idempotent(
        &self,
        request: AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        validate_apply_evidence(&request)?;
        let expected_revision = i64::try_from(request.command().expected_revision.get())
            .map_err(|_| invalid_apply_result())?;
        let authority_revision =
            i64::try_from(request.evidence().installation_authority_revision().get())
                .map_err(|_| invalid_apply_result())?;
        let digests = apply_digests(self.config.keyring(), &request);
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self
                .apply_once(&request, &digests, expected_revision, authority_revision)
                .await
            {
                Ok(receipt) => return Ok(receipt),
                Err(ApplyAttemptFailure::Control(error)) => return Err(error),
                Err(ApplyAttemptFailure::Retryable(_))
                    if attempt + 1 < MAX_TRANSACTION_ATTEMPTS => {}
                Err(ApplyAttemptFailure::Retryable(error)) => {
                    return Err(database_backend(error));
                }
            }
        }
        Err(invalid_apply_result())
    }
}

impl PostgresProductDecisions {
    async fn apply_once(
        &self,
        request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
        digests: &ApplyDigests,
        expected_revision: i64,
        authority_revision: i64,
    ) -> Result<ProductMutationReceiptV1, ApplyAttemptFailure> {
        let mut transaction = self
            .pools
            .apply_executor
            .begin()
            .await
            .map_err(classify_precommit_failure)?;
        if let Err(error) = configure_apply_transaction(&mut transaction, &self.config).await {
            let _ = transaction.rollback().await;
            return Err(classify_precommit_failure(error));
        }
        let locked = match lock_apply(
            &mut transaction,
            request,
            digests,
            expected_revision,
            authority_revision,
        )
        .await
        {
            Ok(locked) => locked,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(classify_precommit_failure(error));
            }
        };
        if matches!(locked.outcome.as_str(), "ready" | "ok" | "superseded") {
            let artifacts = match load_apply_target_artifact(&mut transaction, request).await {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(classify_precommit_failure(error));
                }
            };
            match artifacts.as_slice() {
                [artifact] if target_artifact_is_valid(artifact) => {}
                [] | [_] => {
                    let _ = transaction.rollback().await;
                    return Err(ApplyAttemptFailure::Control(target_corrupt()));
                }
                _ => {
                    let _ = transaction.rollback().await;
                    return Err(ApplyAttemptFailure::Control(invalid_apply_result()));
                }
            }
        }
        if locked.outcome == "ok" || locked.outcome == "superseded" {
            let receipt = replay_or_terminal_receipt(request, &locked)?;
            commit_apply(transaction).await?;
            return Ok(receipt);
        }
        if locked.outcome != "ready" {
            let error = map_lock_outcome(&locked.outcome);
            let _ = transaction.rollback().await;
            return Err(ApplyAttemptFailure::Control(error));
        }
        validate_ready_lock(&locked, digests, expected_revision)?;
        let locked_projection = locked
            .locked_projection
            .as_ref()
            .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?;
        let prepared = match prepare_product_apply_v1(locked_projection.0.clone(), request, digests)
        {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(ApplyAttemptFailure::Control(error));
            }
        };
        let finalized = match finalize_apply(
            &mut transaction,
            request,
            digests,
            expected_revision,
            authority_revision,
            &locked_projection.0,
            &prepared,
        )
        .await
        {
            Ok(finalized) => finalized,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(classify_precommit_failure(error));
            }
        };
        let receipt = finalized_receipt(request, digests, &prepared, finalized)?;
        commit_apply(transaction).await?;
        Ok(receipt)
    }
}

fn validate_apply_evidence(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), ProductControlPortError> {
    let evidence = request.evidence();
    if evidence.capability() != CapabilityV1::Apply {
        return Err(ProductControlPortError::InvalidState);
    }
    if evidence.tenant_id() != request.scope().tenant_id()
        || evidence.installation_id() != request.scope().installation_id()
        || evidence.guild_id() != request.scope().guild_id()
        || evidence.acting_user_id() != request.scope().acting_user_id()
    {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    let runtime = evidence
        .apply_runtime_environment()
        .ok_or(ProductControlPortError::InvalidState)?;
    if runtime.guild_id() != request.scope().guild_id() {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    Ok(())
}

fn validate_ready_lock(
    locked: &ApplyLockRow,
    digests: &ApplyDigests,
    expected_revision: i64,
) -> Result<(), ApplyAttemptFailure> {
    if locked.exact_replay
        || locked.requires_commit
        || locked.resulting_revision != Some(expected_revision)
        || locked.resulting_state.as_deref() != Some("approved")
        || locked.deployment_id.as_deref() != Some(digests.deployment_id.as_str())
        || locked.desired_target_digest.is_some()
        || locked.locked_projection.is_none()
    {
        return Err(ApplyAttemptFailure::Control(invalid_apply_result()));
    }
    Ok(())
}

fn replay_or_terminal_receipt(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    locked: &ApplyLockRow,
) -> Result<ProductMutationReceiptV1, ApplyAttemptFailure> {
    if !locked.requires_commit || locked.locked_projection.is_some() {
        return Err(ApplyAttemptFailure::Control(invalid_apply_result()));
    }
    let revision = revision_from_database(locked.resulting_revision)?;
    let state = locked
        .resulting_state
        .as_deref()
        .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?;
    let phase = match state {
        "applied" if locked.outcome == "ok" && locked.exact_replay => {
            let deployment_id = locked
                .deployment_id
                .as_deref()
                .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?;
            let desired_target_digest = locked
                .desired_target_digest
                .as_deref()
                .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?;
            ProductDecisionPhaseV1::Applied {
                exact_deployment: exact_deployment(request, deployment_id, desired_target_digest)?,
            }
        }
        "superseded"
            if locked.outcome == "superseded"
                && locked.deployment_id.is_none()
                && locked.desired_target_digest.is_none() =>
        {
            ProductDecisionPhaseV1::Superseded
        }
        _ => return Err(ApplyAttemptFailure::Control(invalid_apply_result())),
    };
    Ok(mutation_receipt(
        request,
        revision,
        phase,
        locked.exact_replay,
    ))
}

fn finalized_receipt(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    prepared: &super::apply_projection::PreparedProductApplyV1,
    finalized: ApplyFinalizeRow,
) -> Result<ProductMutationReceiptV1, ApplyAttemptFailure> {
    let expected_revision = request
        .command()
        .expected_revision
        .get()
        .checked_add(2)
        .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?;
    if finalized.outcome == "superseded" {
        if !is_terminal_supersession(&finalized) {
            return Err(ApplyAttemptFailure::Control(invalid_apply_result()));
        }
        let revision = ProductRevisionV1::new(expected_revision)
            .map_err(|_| ApplyAttemptFailure::Control(invalid_apply_result()))?;
        return Ok(mutation_receipt(
            request,
            revision,
            ProductDecisionPhaseV1::Superseded,
            false,
        ));
    }
    let revision = revision_from_database(finalized.resulting_revision)?;
    let expected_guild_id = request.scope().guild_id().to_string();
    if finalized.outcome != "ok"
        || finalized.resulting_state.as_deref() != Some("applied")
        || finalized.exact_replay
        || finalized.guild_id.as_deref() != Some(expected_guild_id.as_str())
        || finalized.deployment_id.as_deref() != Some(digests.deployment_id.as_str())
        || finalized.desired_target_digest.as_deref()
            != Some(prepared.deployment.desired_target_digest())
        || revision.get() != expected_revision
    {
        return Err(ApplyAttemptFailure::Control(map_finalize_outcome(
            &finalized.outcome,
        )));
    }
    let exact = exact_deployment(
        request,
        finalized
            .deployment_id
            .as_deref()
            .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?,
        finalized
            .desired_target_digest
            .as_deref()
            .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?,
    )?;
    Ok(mutation_receipt(
        request,
        revision,
        ProductDecisionPhaseV1::Applied {
            exact_deployment: exact,
        },
        false,
    ))
}

fn is_terminal_supersession(finalized: &ApplyFinalizeRow) -> bool {
    finalized.resulting_revision.is_none()
        && finalized.resulting_state.is_none()
        && !finalized.exact_replay
        && finalized.guild_id.is_none()
        && finalized.deployment_id.is_none()
        && finalized.desired_target_digest.is_none()
}

fn exact_deployment(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    deployment_id: &str,
    target_digest: &str,
) -> Result<ExactDeploymentSelectorV1, ApplyAttemptFailure> {
    ExactDeploymentSelectorV1::from_server_projection(
        request.scope().installation_id().clone(),
        request.command().promotion.promotion_id().clone(),
        deployment_id,
        target_digest,
    )
    .map_err(|_| ApplyAttemptFailure::Control(invalid_apply_result()))
}

fn mutation_receipt(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    revision: ProductRevisionV1,
    phase: ProductDecisionPhaseV1,
    exact_replay: bool,
) -> ProductMutationReceiptV1 {
    ProductMutationReceiptV1::from_server_projection(
        ProductDecisionProjectionV1::from_server_projection(
            request.scope().tenant_id().clone(),
            request.scope().installation_id().clone(),
            request.scope().guild_id(),
            request.command().promotion.promotion_id().clone(),
            revision,
            phase,
        ),
        exact_replay,
    )
}

fn revision_from_database(revision: Option<i64>) -> Result<ProductRevisionV1, ApplyAttemptFailure> {
    let revision = revision
        .and_then(|revision| u64::try_from(revision).ok())
        .and_then(|revision| ProductRevisionV1::new(revision).ok())
        .ok_or_else(|| ApplyAttemptFailure::Control(invalid_apply_result()))?;
    Ok(revision)
}

fn map_lock_outcome(outcome: &str) -> ProductControlPortError {
    match outcome {
        "not_found" => ProductControlPortError::NotFound,
        "scope_mismatch" => ProductControlPortError::ScopeMismatch,
        "revision_conflict" => ProductControlPortError::RevisionConflict,
        "payload_mismatch" => ProductControlPortError::PayloadMismatch,
        "expired" => ProductControlPortError::Expired,
        "idempotency_conflict" => ProductControlPortError::IdempotencyConflict,
        "invalid_state"
        | "authorization_stale"
        | "authority_mismatch"
        | "baseline_mismatch"
        | "runtime_pending_conflict" => ProductControlPortError::InvalidState,
        "target_mismatch" => ProductControlPortError::InvalidServerCandidate(
            authoring_application::ProductCandidateErrorCodeV1::TargetCorrupt,
        ),
        "indeterminate" => ProductControlPortError::Indeterminate(
            "persisted product apply receipt is incomplete".to_string(),
        ),
        "idempotency_keyring_incomplete" => ProductControlPortError::Backend(
            "product apply idempotency keyring does not cover live receipts".to_string(),
        ),
        _ => invalid_apply_result(),
    }
}

fn map_finalize_outcome(outcome: &str) -> ProductControlPortError {
    if outcome == "ok" {
        invalid_apply_result()
    } else {
        map_lock_outcome(outcome)
    }
}

async fn commit_apply(
    transaction: sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), ApplyAttemptFailure> {
    match transaction.commit().await {
        Ok(()) => Ok(()),
        Err(error) if commit_failure_proves_rollback(&error) => {
            Err(ApplyAttemptFailure::Retryable(error))
        }
        Err(error) => Err(ApplyAttemptFailure::Control(database_commit(
            error,
            "product apply commit outcome is unavailable",
        ))),
    }
}

fn classify_precommit_failure(error: sqlx::Error) -> ApplyAttemptFailure {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .as_deref()
        == Some("PZ012")
    {
        ApplyAttemptFailure::Control(target_corrupt())
    } else if is_safe_transaction_retry(&error) {
        ApplyAttemptFailure::Retryable(error)
    } else {
        ApplyAttemptFailure::Control(database_backend(error))
    }
}

fn target_artifact_is_valid(artifact: &ApplyTargetArtifactRow) -> bool {
    let Some(schema_version) = u32::try_from(artifact.schema_version)
        .ok()
        .and_then(|value| RuleSetSchemaVersion::new(value).ok())
    else {
        return false;
    };
    if schema_version != CURRENT_RULESET_SCHEMA_VERSION {
        return false;
    }
    let Some(definition) = artifact.definition.as_ref().and_then(|definition| {
        serde_json::from_value::<InteractionRuleSet>(definition.0.clone()).ok()
    }) else {
        return false;
    };
    let Some(persisted_hash) = RuleSetContentHash::parse_hex(&artifact.content_hash) else {
        return false;
    };
    artifact.canonical_content_hash.as_deref() == Some(artifact.content_hash.as_str())
        && automation_core::validate_structural(&definition).is_ok()
        && content_hash(schema_version, &definition).ok() == Some(persisted_hash)
}

fn target_corrupt() -> ProductControlPortError {
    ProductControlPortError::InvalidServerCandidate(
        authoring_application::ProductCandidateErrorCodeV1::TargetCorrupt,
    )
}

fn invalid_apply_result() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "product apply function returned an invalid result".to_string(),
    )
}

enum ApplyAttemptFailure {
    Retryable(sqlx::Error),
    Control(ProductControlPortError),
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{content_hash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_state::InteractionRuleSet;
    use sqlx::types::Json;

    use super::{
        is_terminal_supersession, target_artifact_is_valid, ApplyFinalizeRow,
        ApplyTargetArtifactRow,
    };

    fn artifact(schema_version: RuleSetSchemaVersion) -> ApplyTargetArtifactRow {
        let definition = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        };
        let content_hash = content_hash(schema_version, &definition).unwrap().to_hex();
        ApplyTargetArtifactRow {
            schema_version: i64::from(schema_version.get()),
            definition: Some(Json(serde_json::to_value(definition).unwrap())),
            content_hash: content_hash.clone(),
            canonical_content_hash: Some(content_hash),
        }
    }

    #[test]
    fn finalizer_supersession_requires_the_exact_terminal_shape() {
        let terminal = ApplyFinalizeRow {
            outcome: "superseded".to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
            deployment_id: None,
            desired_target_digest: None,
        };
        assert!(is_terminal_supersession(&terminal));
        assert!(!is_terminal_supersession(&ApplyFinalizeRow {
            exact_replay: true,
            ..terminal
        }));
    }

    #[test]
    fn artifact_verifier_rejects_self_consistent_unsupported_schema() {
        assert!(target_artifact_is_valid(&artifact(
            CURRENT_RULESET_SCHEMA_VERSION
        )));
        assert!(!target_artifact_is_valid(&artifact(
            RuleSetSchemaVersion::new(CURRENT_RULESET_SCHEMA_VERSION.get() + 1).unwrap()
        )));
    }
}
