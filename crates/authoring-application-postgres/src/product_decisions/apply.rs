use std::num::NonZeroU64;

use authoring_application::{
    AuthorizedApplyProductV1, CapabilityV1, ExactDeploymentSelectorV1, ProductApplyPort,
    ProductControlPortError, ProductDecisionPhaseV1, ProductDecisionProjectionV1,
    ProductMutationReceiptV1, ProductRevisionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_controller::{
    RuntimeCanonicalDrainIntentStateV2, RuntimeDrainIntentCanonicalStateKindV2,
    RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2, RuntimePersistedProductDrainRootV2,
    RuntimeProductDrainScopeLookupV2, RuntimeProductMutationDigestV2, RuntimeProductMutationKindV2,
    RuntimeProductOperationIdV2, RuntimeUnixMicrosecondsV2,
};
use automation_runtime_convergence::{
    RuntimeDeployment, RuntimeDeploymentPhaseKindV1, RuntimeDeploymentSnapshotV1,
    RuntimeDeploymentTargetV1,
};
use automation_state::InteractionRuleSet;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::apply_projection::prepare_product_apply_v1;
use super::apply_sql::{
    begin_runtime_drain, finalize_apply, load_apply_target_artifact, lock_apply,
    ApplyBeginRuntimeDrainRow, ApplyFinalizeRow, ApplyLockRow, ApplyTargetArtifactRow,
};
use super::database::{
    commit_failure_proves_rollback, configure_apply_transaction, database_backend, database_commit,
    is_safe_transaction_retry,
};
use super::digest::{apply_digests, ApplyDigests};
use super::runtime_identity::RuntimeDrainCandidateIdsV2;
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
        if locked.outcome == "runtime_drain_required" {
            validate_runtime_drain_lock(&locked)?;
            let observed = match begin_runtime_drain(
                &mut transaction,
                request,
                digests,
                None,
                expected_revision,
                authority_revision,
            )
            .await
            {
                Ok(observed) => observed,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(classify_precommit_failure(error));
                }
            };
            let disposition =
                match validate_runtime_drain_observation(request, digests, None, &observed) {
                    Ok(disposition) => disposition,
                    Err(error) => {
                        let _ = transaction.rollback().await;
                        return Err(error);
                    }
                };
            if disposition == RuntimeDrainStartDispositionV2::Absent {
                let candidates = match RuntimeDrainCandidateIdsV2::generate() {
                    Ok(candidates) => candidates,
                    Err(error) => {
                        let _ = transaction.rollback().await;
                        return Err(ApplyAttemptFailure::Control(error));
                    }
                };
                let started = match begin_runtime_drain(
                    &mut transaction,
                    request,
                    digests,
                    Some(&candidates),
                    expected_revision,
                    authority_revision,
                )
                .await
                {
                    Ok(started) => started,
                    Err(error) => {
                        let _ = transaction.rollback().await;
                        return Err(classify_precommit_failure(error));
                    }
                };
                if started.outcome != "inserted"
                    || !matches!(
                        validate_runtime_drain_observation(
                            request,
                            digests,
                            Some(&candidates),
                            &started,
                        ),
                        Ok(RuntimeDrainStartDispositionV2::Present)
                    )
                {
                    let _ = transaction.rollback().await;
                    return Err(ApplyAttemptFailure::Control(invalid_apply_result()));
                }
            }
            commit_apply(transaction).await?;
            return Err(ApplyAttemptFailure::Control(
                ProductControlPortError::RuntimeDrainRequired,
            ));
        }
        if locked.outcome != "ready" {
            let error = map_lock_failure(&locked);
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

fn validate_runtime_drain_lock(locked: &ApplyLockRow) -> Result<(), ApplyAttemptFailure> {
    if map_lock_failure(locked) == ProductControlPortError::RuntimeDrainRequired {
        Ok(())
    } else {
        Err(ApplyAttemptFailure::Control(invalid_apply_result()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDrainStartDispositionV2 {
    Absent,
    Present,
}

struct ValidatedRuntimeDrainScopeV2 {
    locked_snapshot: RuntimeDeploymentSnapshotV1,
    lookup: RuntimeProductDrainScopeLookupV2,
    expected_target: RuntimeDeploymentTargetV1,
    expected_revision: i64,
    observed_at: DateTime<Utc>,
}

fn validate_runtime_drain_observation(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    candidates: Option<&RuntimeDrainCandidateIdsV2>,
    started: &ApplyBeginRuntimeDrainRow,
) -> Result<RuntimeDrainStartDispositionV2, ApplyAttemptFailure> {
    match started.outcome.as_str() {
        "absent" => {
            if candidates.is_some() {
                return Err(invalid_runtime_drain_attempt());
            }
            let scope = validate_runtime_drain_scope(request, digests, started)?;
            if !runtime_drain_absence_is_exact(started, &scope) {
                return Err(invalid_runtime_drain_attempt());
            }
            Ok(RuntimeDrainStartDispositionV2::Absent)
        }
        "inserted" | "replayed" => {
            validate_present_runtime_drain(request, digests, candidates, started)?;
            Ok(RuntimeDrainStartDispositionV2::Present)
        }
        _ if runtime_drain_start_projection_is_empty(started) => Err(ApplyAttemptFailure::Control(
            map_runtime_drain_start_outcome(&started.outcome),
        )),
        _ => Err(invalid_runtime_drain_attempt()),
    }
}

fn validate_runtime_drain_scope(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    started: &ApplyBeginRuntimeDrainRow,
) -> Result<ValidatedRuntimeDrainScopeV2, ApplyAttemptFailure> {
    let locked_snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(
        started
            .locked_snapshot
            .as_ref()
            .ok_or_else(invalid_runtime_drain_attempt)?
            .0
            .clone(),
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    RuntimeDeployment::restore(locked_snapshot.clone())
        .map_err(|_| invalid_runtime_drain_attempt())?;
    if locked_snapshot.identity.deployment_id.as_str() == digests.deployment_id
        || locked_snapshot.identity.tenant_id.as_str() != request.scope().tenant_id().as_str()
        || locked_snapshot.identity.installation_id.as_str()
            != request.scope().installation_id().as_str()
        || locked_snapshot.target.guild_id != request.scope().guild_id()
        || !matches!(
            locked_snapshot.phase.kind(),
            RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady | RuntimeDeploymentPhaseKindV1::Live
        )
    {
        return Err(invalid_runtime_drain_attempt());
    }
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(&locked_snapshot)
        .map_err(|_| invalid_runtime_drain_attempt())?;
    let product_scope = lookup.product_operation_scope();
    let drain_scope = lookup.drain_intent_scope();
    let expected_revision = i64::try_from(product_scope.expected_revision().get())
        .map_err(|_| invalid_runtime_drain_attempt())?;
    let expected_target = serde_json::from_value::<RuntimeDeploymentTargetV1>(
        started
            .product_expected_target
            .as_ref()
            .ok_or_else(invalid_runtime_drain_attempt)?
            .0
            .clone(),
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let observed_at = started
        .observed_at
        .ok_or_else(invalid_runtime_drain_attempt)?;
    RuntimeUnixMicrosecondsV2::from_datetime(observed_at)
        .map_err(|_| invalid_runtime_drain_attempt())?;
    if started.product_tenant_id.as_deref() != Some(product_scope.scope().tenant_id.as_str())
        || started.product_installation_id.as_deref()
            != Some(product_scope.scope().installation_id.as_str())
        || started.product_deployment_id.as_deref()
            != Some(product_scope.scope().deployment_id.as_str())
        || started.product_expected_revision != Some(expected_revision)
        || started.drain_tenant_id.as_deref() != Some(drain_scope.scope().tenant_id.as_str())
        || started.drain_installation_id.as_deref()
            != Some(drain_scope.scope().installation_id.as_str())
        || started.drain_deployment_id.as_deref()
            != Some(drain_scope.scope().deployment_id.as_str())
        || started.drain_slot_guild_id.as_deref()
            != Some(drain_scope.slot().guild_id.to_string().as_str())
        || started.drain_slot_ruleset_key.as_deref()
            != Some(drain_scope.slot().ruleset_key.as_str())
        || started.drain_expected_revision != Some(expected_revision)
        || expected_target != locked_snapshot.target
    {
        return Err(invalid_runtime_drain_attempt());
    }
    Ok(ValidatedRuntimeDrainScopeV2 {
        locked_snapshot,
        lookup,
        expected_target,
        expected_revision,
        observed_at,
    })
}

fn runtime_drain_absence_is_exact(
    started: &ApplyBeginRuntimeDrainRow,
    scope: &ValidatedRuntimeDrainScopeV2,
) -> bool {
    started.product_operation_id.is_none()
        && started.product_mutation_request_bytes.is_none()
        && started.product_mutation_digest.is_none()
        && started.drain_intent_id.is_none()
        && started.drain_intent_request_bytes.is_none()
        && started.drain_intent_digest.is_none()
        && started.intent_revision.is_none()
        && started.intent_state.is_none()
        && started.canonical_state_bytes.is_none()
        && started.canonical_state_digest.is_none()
        && started.writer_epoch_before.is_some_and(|epoch| epoch > 0)
        && started.writer_epoch_before == started.writer_epoch_after
        && started.pending_drain_intent_id.is_none()
        && started.pending_product_operation_id.is_none()
        && started.pending_tenant_id.is_none()
        && started.pending_installation_id.is_none()
        && started.pending_deployment_id.is_none()
        && started.pending_expected_revision.is_none()
        && started.pending_marked_at.is_none()
        && scope.expected_target == scope.locked_snapshot.target
}

fn validate_present_runtime_drain(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    candidates: Option<&RuntimeDrainCandidateIdsV2>,
    started: &ApplyBeginRuntimeDrainRow,
) -> Result<(), ApplyAttemptFailure> {
    let scope = validate_runtime_drain_scope(request, digests, started)?;
    let product_scope = scope.lookup.product_operation_scope();
    let drain_scope = scope.lookup.drain_intent_scope();
    let product_operation_id = RuntimeProductOperationIdV2::parse(
        started
            .product_operation_id
            .clone()
            .ok_or_else(invalid_runtime_drain_attempt)?,
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let drain_intent_id = RuntimeDrainIntentIdV2::parse(
        started
            .drain_intent_id
            .clone()
            .ok_or_else(invalid_runtime_drain_attempt)?,
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let product_mutation_digest = RuntimeProductMutationDigestV2::parse(
        started
            .product_mutation_digest
            .clone()
            .ok_or_else(invalid_runtime_drain_attempt)?,
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let drain_intent_digest = RuntimeDrainIntentDigestV2::parse(
        started
            .drain_intent_digest
            .clone()
            .ok_or_else(invalid_runtime_drain_attempt)?,
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let persisted = RuntimePersistedProductDrainRootV2::from_persisted(
        product_scope.scope().clone(),
        product_scope.expected_revision(),
        &product_operation_id,
        drain_scope.scope().clone(),
        drain_scope.slot().clone(),
        drain_scope.expected_revision(),
        &drain_intent_id,
        &scope.expected_target,
        started
            .product_mutation_request_bytes
            .as_deref()
            .ok_or_else(invalid_runtime_drain_attempt)?,
        &product_mutation_digest,
        started
            .drain_intent_request_bytes
            .as_deref()
            .ok_or_else(invalid_runtime_drain_attempt)?,
        &drain_intent_digest,
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let product = persisted.canonical().product_preimage();
    let intent_revision = started
        .intent_revision
        .and_then(|revision| u64::try_from(revision).ok())
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid_runtime_drain_attempt)?;
    let canonical_state_bytes = started
        .canonical_state_bytes
        .as_deref()
        .ok_or_else(invalid_runtime_drain_attempt)?;
    let canonical_state_digest = started
        .canonical_state_digest
        .as_deref()
        .ok_or_else(invalid_runtime_drain_attempt)?;
    let canonical_state = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &persisted,
        intent_revision,
        started
            .intent_state
            .as_deref()
            .ok_or_else(invalid_runtime_drain_attempt)?,
        canonical_state_bytes,
    )
    .map_err(|_| invalid_runtime_drain_attempt())?;
    let state_kind = canonical_state
        .state_kind()
        .map_err(|_| invalid_runtime_drain_attempt())?;
    let intent_progress_is_valid = match started.outcome.as_str() {
        "inserted" => {
            intent_revision == NonZeroU64::MIN
                && state_kind == RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed
        }
        "replayed" => matches!(
            state_kind,
            RuntimeDrainIntentCanonicalStateKindV2::PendingUnclaimed
                | RuntimeDrainIntentCanonicalStateKindV2::PendingClaimed
                | RuntimeDrainIntentCanonicalStateKindV2::PendingRefenced
                | RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
        ),
        _ => false,
    };
    let epoch_progress_is_valid = match started.outcome.as_str() {
        "inserted" => started.writer_epoch_before.is_some_and(|before| {
            before > 0
                && before < i64::MAX
                && started.writer_epoch_after == Some(before.saturating_add(1))
        }),
        "replayed" => {
            started.writer_epoch_before.is_some_and(|before| before > 1)
                && started.writer_epoch_before == started.writer_epoch_after
        }
        _ => false,
    };
    let pending_marked_at = started
        .pending_marked_at
        .ok_or_else(invalid_runtime_drain_attempt)?;
    RuntimeUnixMicrosecondsV2::from_datetime(pending_marked_at)
        .map_err(|_| invalid_runtime_drain_attempt())?;
    let time_progress_is_valid = match started.outcome.as_str() {
        "inserted" => pending_marked_at >= scope.observed_at,
        "replayed" => pending_marked_at <= scope.observed_at,
        _ => false,
    };
    let inserted_identity_is_valid = started.outcome != "inserted"
        || candidates.is_some_and(|candidates| {
            product_operation_id.as_str() == candidates.product_operation_id
                && drain_intent_id.as_str() == candidates.drain_intent_id
        });
    if product_operation_id.as_str() == drain_intent_id.as_str()
        || product.expected_target != scope.locked_snapshot.target
        || product.mutation_kind != RuntimeProductMutationKindV2::Apply
        || product.product_semantic_request_digest.as_str() != digests.semantic_request
        || !state_digest_matches(canonical_state_bytes, canonical_state_digest)
        || started.pending_drain_intent_id.as_deref() != Some(drain_intent_id.as_str())
        || started.pending_product_operation_id.as_deref() != Some(product_operation_id.as_str())
        || started.pending_tenant_id.as_deref() != Some(product_scope.scope().tenant_id.as_str())
        || started.pending_installation_id.as_deref()
            != Some(product_scope.scope().installation_id.as_str())
        || started.pending_deployment_id.as_deref()
            != Some(product_scope.scope().deployment_id.as_str())
        || started.pending_expected_revision != Some(scope.expected_revision)
        || !time_progress_is_valid
        || !inserted_identity_is_valid
        || !intent_progress_is_valid
        || !epoch_progress_is_valid
        || !started.writer_epoch_after.is_some_and(|epoch| epoch > 1)
    {
        return Err(ApplyAttemptFailure::Control(invalid_apply_result()));
    }
    Ok(())
}

fn invalid_runtime_drain_attempt() -> ApplyAttemptFailure {
    ApplyAttemptFailure::Control(invalid_apply_result())
}

fn state_digest_matches(state_bytes: &[u8], expected_digest: &str) -> bool {
    if expected_digest.len() != 64 {
        return false;
    }
    let digest = Sha256::digest(state_bytes);
    digest.iter().enumerate().all(|(index, byte)| {
        let high = lower_hex_digit(byte >> 4);
        let low = lower_hex_digit(byte & 0x0f);
        expected_digest.as_bytes().get(index * 2) == Some(&high)
            && expected_digest.as_bytes().get(index * 2 + 1) == Some(&low)
    })
}

fn lower_hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        10..=15 => b'a' + value - 10,
        _ => unreachable!(),
    }
}

fn runtime_drain_start_projection_is_empty(started: &ApplyBeginRuntimeDrainRow) -> bool {
    started.locked_snapshot.is_none()
        && started.observed_at.is_none()
        && started.product_tenant_id.is_none()
        && started.product_installation_id.is_none()
        && started.product_deployment_id.is_none()
        && started.product_expected_revision.is_none()
        && started.product_operation_id.is_none()
        && started.product_expected_target.is_none()
        && started.product_mutation_request_bytes.is_none()
        && started.product_mutation_digest.is_none()
        && started.drain_tenant_id.is_none()
        && started.drain_installation_id.is_none()
        && started.drain_deployment_id.is_none()
        && started.drain_slot_guild_id.is_none()
        && started.drain_slot_ruleset_key.is_none()
        && started.drain_expected_revision.is_none()
        && started.drain_intent_id.is_none()
        && started.drain_intent_request_bytes.is_none()
        && started.drain_intent_digest.is_none()
        && started.intent_revision.is_none()
        && started.intent_state.is_none()
        && started.canonical_state_bytes.is_none()
        && started.canonical_state_digest.is_none()
        && started.writer_epoch_before.is_none()
        && started.writer_epoch_after.is_none()
        && started.pending_drain_intent_id.is_none()
        && started.pending_product_operation_id.is_none()
        && started.pending_tenant_id.is_none()
        && started.pending_installation_id.is_none()
        && started.pending_deployment_id.is_none()
        && started.pending_expected_revision.is_none()
        && started.pending_marked_at.is_none()
}

fn map_runtime_drain_start_outcome(outcome: &str) -> ProductControlPortError {
    match outcome {
        "not_found" => ProductControlPortError::NotFound,
        "scope_mismatch" => ProductControlPortError::ScopeMismatch,
        "revision_conflict" => ProductControlPortError::RevisionConflict,
        "payload_mismatch" => ProductControlPortError::PayloadMismatch,
        "expired" => ProductControlPortError::Expired,
        "idempotency_conflict" => ProductControlPortError::IdempotencyConflict,
        "authorization_stale"
        | "authority_mismatch"
        | "invalid_input"
        | "invalid_state"
        | "runtime_pending_conflict"
        | "deployment_mismatch"
        | "slot_conflict" => ProductControlPortError::InvalidState,
        "runtime_writer_fenced" => {
            ProductControlPortError::Backend("product apply is temporarily unavailable".to_string())
        }
        "idempotency_keyring_incomplete" => ProductControlPortError::Backend(
            "product apply idempotency keyring does not cover live receipts".to_string(),
        ),
        "indeterminate" => ProductControlPortError::Indeterminate(
            "runtime drain creation outcome is unavailable".to_string(),
        ),
        "persistence_corrupt" | "diverged" | "identifier_conflict" => {
            ProductControlPortError::Backend(
                "runtime drain creation returned an invalid persisted state".to_string(),
            )
        }
        _ => invalid_apply_result(),
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
    if finalized.outcome == "runtime_drain_required" {
        return Err(ApplyAttemptFailure::Control(map_runtime_drain_finalize(
            &finalized,
        )));
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
    finalize_row_is_closed(finalized)
}

fn finalize_row_is_closed(finalized: &ApplyFinalizeRow) -> bool {
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
        "runtime_writer_fenced" => {
            ProductControlPortError::Backend("product apply is temporarily unavailable".to_string())
        }
        "runtime_writer_fence_invalid" => {
            ProductControlPortError::Backend("runtime writer fence is unavailable".to_string())
        }
        _ => invalid_apply_result(),
    }
}

fn map_lock_failure(locked: &ApplyLockRow) -> ProductControlPortError {
    if locked.outcome != "runtime_drain_required" {
        return map_lock_outcome(&locked.outcome);
    }
    if locked.exact_replay
        || locked.requires_commit
        || locked.resulting_revision.is_some()
        || locked.resulting_state.is_some()
        || locked.deployment_id.is_some()
        || locked.desired_target_digest.is_some()
        || locked.locked_projection.is_some()
    {
        return invalid_apply_result();
    }
    ProductControlPortError::RuntimeDrainRequired
}

fn map_finalize_outcome(outcome: &str) -> ProductControlPortError {
    if outcome == "ok" {
        invalid_apply_result()
    } else {
        map_lock_outcome(outcome)
    }
}

fn map_runtime_drain_finalize(finalized: &ApplyFinalizeRow) -> ProductControlPortError {
    if finalize_row_is_closed(finalized) {
        ProductControlPortError::RuntimeDrainRequired
    } else {
        invalid_apply_result()
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
        is_terminal_supersession, map_lock_failure, map_lock_outcome, map_runtime_drain_finalize,
        runtime_drain_start_projection_is_empty, state_digest_matches, target_artifact_is_valid,
        ApplyBeginRuntimeDrainRow, ApplyFinalizeRow, ApplyLockRow, ApplyTargetArtifactRow,
        ProductControlPortError,
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

    fn drain_required_row() -> ApplyLockRow {
        ApplyLockRow {
            outcome: "runtime_drain_required".to_string(),
            exact_replay: false,
            requires_commit: false,
            resulting_revision: None,
            resulting_state: None,
            deployment_id: None,
            desired_target_digest: None,
            locked_projection: None,
        }
    }

    fn drain_required_finalize_row() -> ApplyFinalizeRow {
        ApplyFinalizeRow {
            outcome: "runtime_drain_required".to_string(),
            resulting_revision: None,
            resulting_state: None,
            exact_replay: false,
            guild_id: None,
            deployment_id: None,
            desired_target_digest: None,
        }
    }

    fn failed_runtime_drain(outcome: &str) -> ApplyBeginRuntimeDrainRow {
        ApplyBeginRuntimeDrainRow {
            outcome: outcome.to_string(),
            locked_snapshot: None,
            observed_at: None,
            product_tenant_id: None,
            product_installation_id: None,
            product_deployment_id: None,
            product_expected_revision: None,
            product_operation_id: None,
            product_expected_target: None,
            product_mutation_request_bytes: None,
            product_mutation_digest: None,
            drain_tenant_id: None,
            drain_installation_id: None,
            drain_deployment_id: None,
            drain_slot_guild_id: None,
            drain_slot_ruleset_key: None,
            drain_expected_revision: None,
            drain_intent_id: None,
            drain_intent_request_bytes: None,
            drain_intent_digest: None,
            intent_revision: None,
            intent_state: None,
            canonical_state_bytes: None,
            canonical_state_digest: None,
            writer_epoch_before: None,
            writer_epoch_after: None,
            pending_drain_intent_id: None,
            pending_product_operation_id: None,
            pending_tenant_id: None,
            pending_installation_id: None,
            pending_deployment_id: None,
            pending_expected_revision: None,
            pending_marked_at: None,
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

    #[test]
    fn writer_fence_outcomes_fail_closed_with_stable_product_errors() {
        assert_eq!(
            map_lock_outcome("runtime_writer_fenced"),
            ProductControlPortError::Backend(
                "product apply is temporarily unavailable".to_string()
            )
        );
        assert_eq!(
            map_lock_outcome("runtime_writer_fence_invalid"),
            ProductControlPortError::Backend("runtime writer fence is unavailable".to_string())
        );
    }

    #[test]
    fn runtime_drain_required_requires_the_exact_closed_lock_shape() {
        let locked = drain_required_row();
        assert_eq!(
            map_lock_failure(&locked),
            ProductControlPortError::RuntimeDrainRequired
        );
        for malformed in [
            ApplyLockRow {
                exact_replay: true,
                ..drain_required_row()
            },
            ApplyLockRow {
                requires_commit: true,
                ..drain_required_row()
            },
            ApplyLockRow {
                resulting_revision: Some(1),
                ..drain_required_row()
            },
            ApplyLockRow {
                resulting_state: Some("applied".to_string()),
                ..drain_required_row()
            },
            ApplyLockRow {
                deployment_id: Some("deployment".to_string()),
                ..drain_required_row()
            },
            ApplyLockRow {
                desired_target_digest: Some("digest".to_string()),
                ..drain_required_row()
            },
            ApplyLockRow {
                locked_projection: Some(sqlx::types::Json(serde_json::json!({}))),
                ..drain_required_row()
            },
        ] {
            assert_eq!(
                map_lock_failure(&malformed),
                ProductControlPortError::Backend(
                    "product apply function returned an invalid result".to_string()
                )
            );
        }
        let finalized = drain_required_finalize_row();
        assert_eq!(
            map_runtime_drain_finalize(&finalized),
            ProductControlPortError::RuntimeDrainRequired
        );
        for malformed in [
            ApplyFinalizeRow {
                resulting_revision: Some(1),
                ..drain_required_finalize_row()
            },
            ApplyFinalizeRow {
                resulting_state: Some("applied".to_string()),
                ..drain_required_finalize_row()
            },
            ApplyFinalizeRow {
                exact_replay: true,
                ..drain_required_finalize_row()
            },
            ApplyFinalizeRow {
                guild_id: Some("guild".to_string()),
                ..drain_required_finalize_row()
            },
            ApplyFinalizeRow {
                deployment_id: Some("deployment".to_string()),
                ..drain_required_finalize_row()
            },
            ApplyFinalizeRow {
                desired_target_digest: Some("digest".to_string()),
                ..drain_required_finalize_row()
            },
        ] {
            assert_eq!(
                map_runtime_drain_finalize(&malformed),
                ProductControlPortError::Backend(
                    "product apply function returned an invalid result".to_string()
                )
            );
        }
    }

    #[test]
    fn runtime_drain_state_digest_requires_exact_lowercase_sha256() {
        assert!(state_digest_matches(
            b"state",
            "4ba69735ca53765ed6a709edb56c6ea236b7193a3b29a6b390c346f0f4340e4e"
        ));
        assert!(!state_digest_matches(b"changed", &"0".repeat(64)));
        assert!(!state_digest_matches(b"state", &"A".repeat(64)));
    }

    #[test]
    fn runtime_drain_start_failures_require_an_empty_projection() {
        let failed = failed_runtime_drain("slot_conflict");
        assert!(runtime_drain_start_projection_is_empty(&failed));
        let malformed = ApplyBeginRuntimeDrainRow {
            writer_epoch_after: Some(2),
            ..failed_runtime_drain("slot_conflict")
        };
        assert!(!runtime_drain_start_projection_is_empty(&malformed));
    }
}
