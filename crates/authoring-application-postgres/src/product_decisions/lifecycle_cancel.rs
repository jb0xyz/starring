use std::num::NonZeroU64;

use authoring_application::{
    AuthorizedCancelProductLifecycleV1, CapabilityV1, FreshGuildAuthorityEvidence,
    ProductControlPortError, ProductDecisionPhaseV1, ProductDecisionProjectionV1,
    ProductLifecycleCancellationDeploymentProjectionV1,
    ProductLifecycleCancellationDrainProjectionV1, ProductLifecycleCancellationPort,
    ProductLifecycleCancellationReceiptV1, ProductLifecycleCancellationSlotProjectionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_controller::{
    RuntimeCanonicalDrainIntentStateV2, RuntimeDrainCancellationSourceV2,
    RuntimeDrainIntentCanonicalStateKindV2, RuntimeDrainIntentDigestV2, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentReceiptV2, RuntimePersistedProductDrainRootV2,
    RuntimeProductDrainScopeLookupV2, RuntimeProductMutationDigestV2, RuntimeProductMutationKindV2,
    RuntimeProductOperationIdV2, RuntimeRouteAbsentDrainIntentSourceV2, RuntimeUnixMicrosecondsV2,
};
use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentSnapshotV1,
};
use automation_runtime_convergence_postgres::prepare_product_drain_source_cancellation_v1;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::types::Json;

use super::config::{PostgresProductDecisionsConfig, ProductDecisionConfigError};
use super::database::{
    configure_apply_transaction, database_backend, database_commit, is_safe_transaction_retry,
};
use super::digest::{lifecycle_cancellation_digests, LifecycleCancellationDigests};
use super::row::approval_guild_from_database;
use crate::product_action_digest::ProductActionDigestKeyringV1;

const MAX_TRANSACTION_ATTEMPTS: usize = 2;
const MAX_TERMINAL_PROJECTION_BYTES: usize = 2_097_152;
const CANCEL_RUNTIME_DRAIN_QUERY: &str = "SELECT outcome_name, exact_replay, \
    product_resulting_revision, product_resulting_state, guild_id, product_receipt_id, \
    product_audit_event_id, cancellation_reason_digest, product_operation_id, \
    source_product_mutation_request_bytes, product_mutation_digest, \
    source_drain_intent_request_bytes, drain_intent_digest, source_deployment_id, \
    source_deployment_revision, source_deployment_snapshot, \
    source_deployment_snapshot_digest, source_result_deployment_revision, \
    source_result_deployment_snapshot, source_result_deployment_snapshot_digest, \
    drain_intent_id, source_intent_revision, source_state_bytes, source_state_digest, \
    result_intent_revision, result_intent_state, result_state_bytes, result_state_digest, \
    source_slot_epoch, successor_slot_epoch, terminal_action_id, terminal_projection_bytes, \
    terminal_projection_digest, terminal_database_time \
    FROM public.starring_product_cancel_runtime_drain_v2(\
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
        proposed_terminal_action_id => $29, expected_cancellation_reason => $30, \
        expected_cancellation_reason_digest => $31, expected_drain_intent_id => $32, \
        expected_source_intent_revision => $33, expected_source_state_digest => $34, \
        expected_product_operation_id => $35, expected_source_deployment_revision => $36)";

#[derive(Clone)]
pub struct PostgresProductLifecycleCancellations {
    pub(super) cancellation_executor: PgPool,
    pub(super) config: PostgresProductDecisionsConfig,
}

impl PostgresProductLifecycleCancellations {
    pub fn new(
        cancellation_executor: PgPool,
        keyring: ProductActionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Ok(Self {
            cancellation_executor,
            config: PostgresProductDecisionsConfig::production(keyring)?,
        })
    }

    pub fn with_config(
        cancellation_executor: PgPool,
        config: PostgresProductDecisionsConfig,
    ) -> Self {
        Self {
            cancellation_executor,
            config,
        }
    }

    async fn cancel_once(
        &self,
        request: &AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
        digests: &LifecycleCancellationDigests,
    ) -> Result<ProductLifecycleCancellationReceiptV1, CancellationAttemptFailure> {
        let evidence = request.evidence();
        let selector = &request.command().drain_selector;
        let expected_product_revision = database_input(request.command().expected_revision.get())?;
        let authority_revision = database_input(evidence.installation_authority_revision().get())?;
        let source_intent_revision = database_input(selector.acknowledged_intent_revision().get())?;
        let source_deployment_revision =
            database_input(selector.expected_runtime_deployment_revision().get())?;
        let mut transaction = self
            .cancellation_executor
            .begin()
            .await
            .map_err(classify_precommit_failure)?;
        if let Err(error) = configure_apply_transaction(&mut transaction, &self.config).await {
            let _ = transaction.rollback().await;
            return Err(classify_precommit_failure(error));
        }
        let row = sqlx::query_as::<_, LifecycleCancellationRow>(CANCEL_RUNTIME_DRAIN_QUERY)
            .bind(request.scope().tenant_id().as_str())
            .bind(request.scope().installation_id().as_str())
            .bind(request.command().promotion.promotion_id().as_str())
            .bind(expected_product_revision)
            .bind(request.command().expected_payload_digest.as_str())
            .bind(request.actor().principal_id().as_str())
            .bind(request.session_fingerprint().as_bytes().as_slice())
            .bind(&digests.session_subject)
            .bind(evidence.acting_user_id().to_string())
            .bind(evidence.discord_application_id().get().to_string())
            .bind(evidence.guild_id().to_string())
            .bind("cancel_lifecycle")
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
            .bind(&digests.terminal_action_id)
            .bind(request.command().reason.as_str())
            .bind(&digests.reason_digest)
            .bind(selector.drain_intent_id())
            .bind(source_intent_revision)
            .bind(selector.acknowledged_state_digest())
            .bind(selector.product_operation_id())
            .bind(source_deployment_revision)
            .fetch_one(&mut *transaction)
            .await;
        let row = match row {
            Ok(row) => row,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(classify_precommit_failure(error));
            }
        };
        if !matches!(row.outcome_name.as_str(), "applied" | "replayed") {
            let error = map_failure(&row);
            let _ = transaction.rollback().await;
            return Err(CancellationAttemptFailure::Control(Box::new(error)));
        }
        let receipt = match validate_success(request, digests, &row) {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(CancellationAttemptFailure::Control(Box::new(error)));
            }
        };
        transaction.commit().await.map_err(|error| {
            if is_safe_transaction_retry(&error) {
                CancellationAttemptFailure::Retryable(error)
            } else {
                CancellationAttemptFailure::Control(Box::new(database_commit(
                    error,
                    "Product lifecycle cancellation commit outcome is unavailable",
                )))
            }
        })?;
        Ok(receipt)
    }
}

impl ProductLifecycleCancellationPort<FreshDiscordAuthorityEvidenceV1>
    for PostgresProductLifecycleCancellations
{
    async fn cancel_lifecycle_idempotent(
        &self,
        request: AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductLifecycleCancellationReceiptV1, ProductControlPortError> {
        validate_evidence(&request)?;
        let digests = lifecycle_cancellation_digests(self.config.keyring(), &request);
        for attempt in 0..MAX_TRANSACTION_ATTEMPTS {
            match self.cancel_once(&request, &digests).await {
                Ok(receipt) => return Ok(receipt),
                Err(CancellationAttemptFailure::Control(error)) => return Err(*error),
                Err(CancellationAttemptFailure::Retryable(_))
                    if attempt + 1 < MAX_TRANSACTION_ATTEMPTS => {}
                Err(CancellationAttemptFailure::Retryable(error)) => {
                    return Err(database_backend(error));
                }
            }
        }
        Err(invalid_result())
    }
}

#[derive(sqlx::FromRow)]
struct LifecycleCancellationRow {
    outcome_name: String,
    exact_replay: bool,
    product_resulting_revision: Option<i64>,
    product_resulting_state: Option<String>,
    guild_id: Option<String>,
    product_receipt_id: Option<String>,
    product_audit_event_id: Option<String>,
    cancellation_reason_digest: Option<String>,
    product_operation_id: Option<String>,
    source_product_mutation_request_bytes: Option<Vec<u8>>,
    product_mutation_digest: Option<String>,
    source_drain_intent_request_bytes: Option<Vec<u8>>,
    drain_intent_digest: Option<String>,
    source_deployment_id: Option<String>,
    source_deployment_revision: Option<i64>,
    source_deployment_snapshot: Option<Json<Box<RawValue>>>,
    source_deployment_snapshot_digest: Option<String>,
    source_result_deployment_revision: Option<i64>,
    source_result_deployment_snapshot: Option<Json<Box<RawValue>>>,
    source_result_deployment_snapshot_digest: Option<String>,
    drain_intent_id: Option<String>,
    source_intent_revision: Option<i64>,
    source_state_bytes: Option<Vec<u8>>,
    source_state_digest: Option<String>,
    result_intent_revision: Option<i64>,
    result_intent_state: Option<String>,
    result_state_bytes: Option<Vec<u8>>,
    result_state_digest: Option<String>,
    source_slot_epoch: Option<i64>,
    successor_slot_epoch: Option<i64>,
    terminal_action_id: Option<String>,
    terminal_projection_bytes: Option<Vec<u8>>,
    terminal_projection_digest: Option<String>,
    terminal_database_time: Option<DateTime<Utc>>,
}

fn validate_evidence(
    request: &AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), ProductControlPortError> {
    let evidence = request.evidence();
    if evidence.capability() != CapabilityV1::CancelLifecycle {
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

fn validate_success(
    request: &AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &LifecycleCancellationDigests,
    row: &LifecycleCancellationRow,
) -> Result<ProductLifecycleCancellationReceiptV1, ProductControlPortError> {
    let selector = &request.command().drain_selector;
    let source_snapshot_bytes = row
        .source_deployment_snapshot
        .as_ref()
        .map(|value| value.0.get().as_bytes())
        .ok_or_else(invalid_result)?;
    let result_snapshot_bytes = row
        .source_result_deployment_snapshot
        .as_ref()
        .map(|value| value.0.get().as_bytes())
        .ok_or_else(invalid_result)?;
    let source_snapshot = decode_snapshot(row.source_deployment_snapshot.as_ref())?;
    let result_snapshot = decode_snapshot(row.source_result_deployment_snapshot.as_ref())?;
    let source_snapshot_digest = row
        .source_deployment_snapshot_digest
        .as_deref()
        .ok_or_else(invalid_result)?;
    if !digest_matches(source_snapshot_bytes, source_snapshot_digest) {
        return Err(invalid_result());
    }
    let source_revision = database_deployment_revision(row.source_deployment_revision)?;
    let source_result_revision =
        database_deployment_revision(row.source_result_deployment_revision)?;
    let source_intent_revision = database_revision(row.source_intent_revision)?;
    let result_intent_revision = database_revision(row.result_intent_revision)?;
    let source_state_bytes = row
        .source_state_bytes
        .as_deref()
        .ok_or_else(invalid_result)?;
    let source_state_digest = row
        .source_state_digest
        .as_deref()
        .ok_or_else(invalid_result)?;
    let result_state_bytes = row
        .result_state_bytes
        .as_deref()
        .ok_or_else(invalid_result)?;
    let result_state_digest = row
        .result_state_digest
        .as_deref()
        .ok_or_else(invalid_result)?;
    let terminal_database_time = row.terminal_database_time.ok_or_else(invalid_result)?;
    RuntimeUnixMicrosecondsV2::from_datetime(terminal_database_time)
        .map_err(|_| invalid_result())?;
    let root = restore_root(request, row, &source_snapshot, source_revision)?;
    let canonical_source = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root,
        source_intent_revision,
        "route_absent_acknowledged",
        source_state_bytes,
    )
    .map_err(|_| invalid_result())?;
    if canonical_source
        .state_kind()
        .map_err(|_| invalid_result())?
        != RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
        || canonical_source.intent().canonical() != root.canonical()
        || !digest_matches(source_state_bytes, source_state_digest)
    {
        return Err(invalid_result());
    }
    let acknowledged_source =
        RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged(canonical_source.intent().clone())
            .map_err(|_| invalid_result())?;
    let acknowledged_at = acknowledged_source
        .source()
        .state()
        .acknowledgement()
        .map(|value| value.acknowledged_at())
        .ok_or_else(invalid_result)?;
    let prepared = prepare_product_drain_source_cancellation_v1(
        source_snapshot.clone(),
        source_revision,
        acknowledged_at,
        terminal_database_time,
    )
    .map_err(|_| invalid_result())?;
    let canonical_result = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &root,
        result_intent_revision,
        row.result_intent_state
            .as_deref()
            .ok_or_else(invalid_result)?,
        result_state_bytes,
    )
    .map_err(|_| invalid_result())?;
    let cancellation_source = RuntimeDrainCancellationSourceV2::from_acknowledged(
        acknowledged_source,
        terminal_database_time,
    )
    .map_err(|_| invalid_result())?;
    RuntimeDrainIntentReceiptV2::cancelled(&cancellation_source, canonical_result.intent().clone())
        .map_err(|_| invalid_result())?;
    let source_slot_epoch = database_revision(row.source_slot_epoch)?;
    let successor_slot_epoch = database_revision(row.successor_slot_epoch)?;
    let expected_product_revision =
        i64::try_from(request.command().expected_revision.get()).map_err(|_| invalid_result())?;
    if row.outcome_name == "applied" && row.exact_replay
        || row.outcome_name == "replayed" && !row.exact_replay
        || row.product_resulting_revision != Some(expected_product_revision)
        || row.product_resulting_state.as_deref() != Some("approved")
        || approval_guild_from_database(row.guild_id.as_deref().ok_or_else(invalid_result)?)?
            != request.scope().guild_id()
        || !action_evidence_matches(row, digests)
        || row.cancellation_reason_digest.as_deref() != Some(digests.reason_digest.as_str())
        || row.product_operation_id.as_deref() != Some(selector.product_operation_id())
        || row.source_deployment_id.as_deref()
            != Some(source_snapshot.identity.deployment_id.as_str())
        || source_snapshot.identity.tenant_id.as_str() != request.scope().tenant_id().as_str()
        || source_snapshot.identity.installation_id.as_str()
            != request.scope().installation_id().as_str()
        || source_snapshot.target.guild_id != request.scope().guild_id()
        || source_revision.get() != selector.expected_runtime_deployment_revision().get()
        || source_snapshot.revision != source_revision
        || source_result_revision != prepared.resulting_revision()
        || result_snapshot != *prepared.snapshot()
        || !digest_matches(
            result_snapshot_bytes,
            row.source_result_deployment_snapshot_digest
                .as_deref()
                .ok_or_else(invalid_result)?,
        )
        || row.drain_intent_id.as_deref() != Some(selector.drain_intent_id())
        || source_intent_revision != selector.acknowledged_intent_revision()
        || source_state_digest != selector.acknowledged_state_digest()
        || result_intent_revision.get() != source_intent_revision.get().checked_add(1).unwrap_or(0)
        || !digest_matches(result_state_bytes, result_state_digest)
        || canonical_result
            .state_kind()
            .map_err(|_| invalid_result())?
            != RuntimeDrainIntentCanonicalStateKindV2::Cancelled
        || successor_slot_epoch.get() != source_slot_epoch.get().checked_add(1).unwrap_or(0)
        || row.terminal_action_id.as_deref() != Some(digests.terminal_action_id.as_str())
        || !row
            .terminal_projection_bytes
            .as_deref()
            .filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_TERMINAL_PROJECTION_BYTES)
            .zip(row.terminal_projection_digest.as_deref())
            .is_some_and(|(bytes, digest)| digest_matches(bytes, digest))
    {
        return Err(invalid_result());
    }
    let decision = ProductDecisionProjectionV1::from_server_projection(
        request.scope().tenant_id().clone(),
        request.scope().installation_id().clone(),
        request.scope().guild_id(),
        request.command().promotion.promotion_id().clone(),
        request.command().expected_revision,
        ProductDecisionPhaseV1::Approved,
    );
    let deployment = ProductLifecycleCancellationDeploymentProjectionV1::from_server_projection(
        source_result_revision.get(),
    )
    .map_err(|_| invalid_result())?;
    let drain = ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
        selector.clone(),
        result_intent_revision.get(),
        result_state_digest,
    )
    .map_err(|_| invalid_result())?;
    let slot = ProductLifecycleCancellationSlotProjectionV1::from_server_projection(
        source_slot_epoch.get(),
        successor_slot_epoch.get(),
    )
    .map_err(|_| invalid_result())?;
    ProductLifecycleCancellationReceiptV1::from_server_projection(
        decision,
        deployment,
        drain,
        slot,
        terminal_database_time.into(),
        row.exact_replay,
    )
    .map_err(|_| invalid_result())
}

fn restore_root(
    request: &AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
    row: &LifecycleCancellationRow,
    source_snapshot: &RuntimeDeploymentSnapshotV1,
    source_revision: DeploymentRevision,
) -> Result<RuntimePersistedProductDrainRootV2, ProductControlPortError> {
    let lookup = RuntimeProductDrainScopeLookupV2::from_locked_snapshot(source_snapshot)
        .map_err(|_| invalid_result())?;
    let product_operation_id = RuntimeProductOperationIdV2::parse(
        row.product_operation_id
            .clone()
            .ok_or_else(invalid_result)?,
    )
    .map_err(|_| invalid_result())?;
    let drain_intent_id =
        RuntimeDrainIntentIdV2::parse(row.drain_intent_id.clone().ok_or_else(invalid_result)?)
            .map_err(|_| invalid_result())?;
    let product_mutation_digest = RuntimeProductMutationDigestV2::parse(
        row.product_mutation_digest
            .clone()
            .ok_or_else(invalid_result)?,
    )
    .map_err(|_| invalid_result())?;
    let drain_intent_digest = RuntimeDrainIntentDigestV2::parse(
        row.drain_intent_digest.clone().ok_or_else(invalid_result)?,
    )
    .map_err(|_| invalid_result())?;
    let root = RuntimePersistedProductDrainRootV2::from_persisted(
        lookup.product_operation_scope().scope().clone(),
        lookup.product_operation_scope().expected_revision(),
        &product_operation_id,
        lookup.drain_intent_scope().scope().clone(),
        lookup.drain_intent_scope().slot().clone(),
        lookup.drain_intent_scope().expected_revision(),
        &drain_intent_id,
        &source_snapshot.target,
        row.source_product_mutation_request_bytes
            .as_deref()
            .ok_or_else(invalid_result)?,
        &product_mutation_digest,
        row.source_drain_intent_request_bytes
            .as_deref()
            .ok_or_else(invalid_result)?,
        &drain_intent_digest,
    )
    .map_err(|_| invalid_result())?;
    let selector = &request.command().drain_selector;
    if lookup.product_operation_scope().expected_revision() != source_revision
        || lookup.drain_intent_scope().expected_revision() != source_revision
        || root.product_operation_id().as_str() != selector.product_operation_id()
        || root.drain_intent_id().as_str() != selector.drain_intent_id()
        || root.canonical().product_preimage().mutation_kind != RuntimeProductMutationKindV2::Apply
    {
        return Err(invalid_result());
    }
    Ok(root)
}

fn decode_snapshot(
    value: Option<&Json<Box<RawValue>>>,
) -> Result<RuntimeDeploymentSnapshotV1, ProductControlPortError> {
    let snapshot = serde_json::from_str::<RuntimeDeploymentSnapshotV1>(
        value.ok_or_else(invalid_result)?.0.get(),
    )
    .map_err(|_| invalid_result())?;
    RuntimeDeployment::restore(snapshot.clone()).map_err(|_| invalid_result())?;
    Ok(snapshot)
}

fn map_failure(row: &LifecycleCancellationRow) -> ProductControlPortError {
    if !row.failure_is_closed() {
        return invalid_result();
    }
    match row.outcome_name.as_str() {
        "invalid_input" | "authorization_stale" | "authority_mismatch" | "terminal_conflict" => {
            ProductControlPortError::InvalidState
        }
        "writer_fenced" => ProductControlPortError::Backend(
            "Product lifecycle cancellation is temporarily unavailable".to_string(),
        ),
        "not_found" => ProductControlPortError::NotFound,
        "scope_mismatch" => ProductControlPortError::ScopeMismatch,
        "revision_conflict" => ProductControlPortError::RevisionConflict,
        "payload_mismatch" => ProductControlPortError::PayloadMismatch,
        "idempotency_conflict" => ProductControlPortError::IdempotencyConflict,
        "idempotency_keyring_incomplete" => ProductControlPortError::Backend(
            "Product lifecycle cancellation keyring does not cover live receipts".to_string(),
        ),
        "persistence_corrupt" => invalid_result(),
        "indeterminate" => ProductControlPortError::Indeterminate(
            "persisted Product lifecycle cancellation receipt is incomplete".to_string(),
        ),
        _ => invalid_result(),
    }
}

impl LifecycleCancellationRow {
    fn failure_is_closed(&self) -> bool {
        !self.exact_replay
            && self.product_resulting_revision.is_none()
            && self.product_resulting_state.is_none()
            && self.guild_id.is_none()
            && self.product_receipt_id.is_none()
            && self.product_audit_event_id.is_none()
            && self.cancellation_reason_digest.is_none()
            && self.product_operation_id.is_none()
            && self.source_product_mutation_request_bytes.is_none()
            && self.product_mutation_digest.is_none()
            && self.source_drain_intent_request_bytes.is_none()
            && self.drain_intent_digest.is_none()
            && self.source_deployment_id.is_none()
            && self.source_deployment_revision.is_none()
            && self.source_deployment_snapshot.is_none()
            && self.source_deployment_snapshot_digest.is_none()
            && self.source_result_deployment_revision.is_none()
            && self.source_result_deployment_snapshot.is_none()
            && self.source_result_deployment_snapshot_digest.is_none()
            && self.drain_intent_id.is_none()
            && self.source_intent_revision.is_none()
            && self.source_state_bytes.is_none()
            && self.source_state_digest.is_none()
            && self.result_intent_revision.is_none()
            && self.result_intent_state.is_none()
            && self.result_state_bytes.is_none()
            && self.result_state_digest.is_none()
            && self.source_slot_epoch.is_none()
            && self.successor_slot_epoch.is_none()
            && self.terminal_action_id.is_none()
            && self.terminal_projection_bytes.is_none()
            && self.terminal_projection_digest.is_none()
            && self.terminal_database_time.is_none()
    }
}

fn database_revision(value: Option<i64>) -> Result<NonZeroU64, ProductControlPortError> {
    value
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid_result)
}

fn database_deployment_revision(
    value: Option<i64>,
) -> Result<DeploymentRevision, ProductControlPortError> {
    value
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(|value| DeploymentRevision::new(value).ok())
        .ok_or_else(invalid_result)
}

fn database_input(value: u64) -> Result<i64, CancellationAttemptFailure> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| CancellationAttemptFailure::Control(Box::new(invalid_result())))
}

fn digest_matches(bytes: &[u8], expected: &str) -> bool {
    expected.len() == 64
        && Sha256::digest(bytes)
            .iter()
            .enumerate()
            .all(|(index, byte)| {
                expected.as_bytes().get(index * 2) == Some(&hex_digit(byte >> 4))
                    && expected.as_bytes().get(index * 2 + 1) == Some(&hex_digit(byte & 0x0f))
            })
}

fn action_evidence_matches(
    row: &LifecycleCancellationRow,
    digests: &LifecycleCancellationDigests,
) -> bool {
    let Some(receipt_id) = row.product_receipt_id.as_deref() else {
        return false;
    };
    let Some(audit_event_id) = row.product_audit_event_id.as_deref() else {
        return false;
    };
    match row.outcome_name.as_str() {
        "applied" => {
            receipt_id == digests.receipt_id
                && audit_event_id == digests.audit_event_id
                && receipt_id != audit_event_id
        }
        "replayed" => digests
            .action_evidence_candidates
            .iter()
            .any(|candidate| candidate.0 == receipt_id && candidate.1 == audit_event_id),
        _ => false,
    }
}

fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        10..=15 => b'a' + value - 10,
        _ => unreachable!(),
    }
}

fn classify_precommit_failure(error: sqlx::Error) -> CancellationAttemptFailure {
    if is_safe_transaction_retry(&error) {
        CancellationAttemptFailure::Retryable(error)
    } else {
        CancellationAttemptFailure::Control(Box::new(database_backend(error)))
    }
}

fn invalid_result() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "Product lifecycle cancellation returned an invalid result".to_string(),
    )
}

enum CancellationAttemptFailure {
    Control(Box<ProductControlPortError>),
    Retryable(sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_contract_selects_all_outputs_and_binds_all_inputs() {
        assert!(CANCEL_RUNTIME_DRAIN_QUERY.starts_with(
            "SELECT outcome_name, exact_replay, product_resulting_revision, \
            product_resulting_state, guild_id"
        ));
        assert!(
            CANCEL_RUNTIME_DRAIN_QUERY.contains("public.starring_product_cancel_runtime_drain_v2(")
        );
        assert!(CANCEL_RUNTIME_DRAIN_QUERY.contains(
            "source_product_mutation_request_bytes, product_mutation_digest, \
            source_drain_intent_request_bytes, drain_intent_digest"
        ));
        assert!(CANCEL_RUNTIME_DRAIN_QUERY.contains(
            "expected_drain_intent_id => $32, expected_source_intent_revision => $33, \
            expected_source_state_digest => $34, expected_product_operation_id => $35, \
            expected_source_deployment_revision => $36"
        ));
        assert_eq!(CANCEL_RUNTIME_DRAIN_QUERY.matches('$').count(), 36);
    }

    #[test]
    fn closed_failure_rows_map_only_declared_outcomes() {
        for (outcome, expected) in [
            ("invalid_input", ProductControlPortError::InvalidState),
            ("authorization_stale", ProductControlPortError::InvalidState),
            ("authority_mismatch", ProductControlPortError::InvalidState),
            ("terminal_conflict", ProductControlPortError::InvalidState),
            ("not_found", ProductControlPortError::NotFound),
            ("scope_mismatch", ProductControlPortError::ScopeMismatch),
            (
                "revision_conflict",
                ProductControlPortError::RevisionConflict,
            ),
            ("payload_mismatch", ProductControlPortError::PayloadMismatch),
            (
                "idempotency_conflict",
                ProductControlPortError::IdempotencyConflict,
            ),
        ] {
            assert_eq!(map_failure(&closed_failure_row(outcome)), expected);
        }
        for outcome in ["writer_fenced", "idempotency_keyring_incomplete"] {
            assert!(matches!(
                map_failure(&closed_failure_row(outcome)),
                ProductControlPortError::Backend(_)
            ));
        }
        assert!(matches!(
            map_failure(&closed_failure_row("indeterminate")),
            ProductControlPortError::Indeterminate(_)
        ));
        for outcome in [
            "persistence_corrupt",
            "applied",
            "replayed",
            "drain_pending",
            "unexpected",
            "",
        ] {
            assert_eq!(map_failure(&closed_failure_row(outcome)), invalid_result());
        }
    }

    #[test]
    fn failure_rows_reject_every_success_projection_field() {
        let mut rows = Vec::new();
        rows.push(LifecycleCancellationRow {
            exact_replay: true,
            ..closed_failure_row("not_found")
        });
        rows.push(LifecycleCancellationRow {
            product_resulting_revision: Some(1),
            ..closed_failure_row("not_found")
        });
        rows.push(LifecycleCancellationRow {
            source_product_mutation_request_bytes: Some(vec![1]),
            ..closed_failure_row("not_found")
        });
        rows.push(LifecycleCancellationRow {
            source_deployment_snapshot: Some(Json(
                RawValue::from_string("null".to_string()).unwrap(),
            )),
            ..closed_failure_row("not_found")
        });
        rows.push(LifecycleCancellationRow {
            result_state_digest: Some("0".repeat(64)),
            ..closed_failure_row("not_found")
        });
        rows.push(LifecycleCancellationRow {
            terminal_projection_bytes: Some(vec![1]),
            ..closed_failure_row("not_found")
        });
        rows.push(LifecycleCancellationRow {
            terminal_database_time: DateTime::from_timestamp_micros(1),
            ..closed_failure_row("not_found")
        });
        for row in rows {
            assert_eq!(map_failure(&row), invalid_result());
        }
    }

    #[test]
    fn database_revisions_and_digests_are_canonical() {
        assert_eq!(database_revision(Some(1)).map(NonZeroU64::get), Ok(1));
        assert_eq!(
            database_deployment_revision(Some(i64::MAX)).map(DeploymentRevision::get),
            Ok(i64::MAX as u64)
        );
        for value in [None, Some(0), Some(-1)] {
            assert_eq!(database_revision(value), Err(invalid_result()));
            assert_eq!(database_deployment_revision(value), Err(invalid_result()));
        }
        let bytes = b"typed lifecycle cancellation";
        let digest = format!("{:x}", Sha256::digest(bytes));
        assert!(digest_matches(bytes, &digest));
        assert!(!digest_matches(bytes, &digest.to_uppercase()));
        assert!(!digest_matches(bytes, "0"));
        assert!(!digest_matches(b"different", &digest));
    }

    #[test]
    fn action_evidence_is_current_on_apply_and_historical_on_replay() {
        let digests = LifecycleCancellationDigests {
            active_idempotency: "1".repeat(64),
            idempotency_candidates: vec!["1".repeat(64)],
            idempotency_candidate_key_ids: vec!["active".to_string()],
            idempotency_candidate_key_fingerprints: vec!["2".repeat(64)],
            active_key_id: "active".to_string(),
            semantic_request: "3".repeat(64),
            receipt_id: "4".repeat(64),
            audit_event_id: "5".repeat(64),
            action_evidence_candidates: vec![
                ("4".repeat(64), "5".repeat(64)),
                ("a".repeat(64), "b".repeat(64)),
            ],
            terminal_action_id: "6".repeat(64),
            reason_digest: "7".repeat(64),
            session_subject: vec![8; 32],
        };
        let mut applied = closed_failure_row("applied");
        applied.product_receipt_id = Some(digests.receipt_id.clone());
        applied.product_audit_event_id = Some(digests.audit_event_id.clone());
        assert!(action_evidence_matches(&applied, &digests));
        applied.product_receipt_id = Some("9".repeat(64));
        assert!(!action_evidence_matches(&applied, &digests));

        let mut replayed = closed_failure_row("replayed");
        replayed.product_receipt_id = Some("a".repeat(64));
        replayed.product_audit_event_id = Some("b".repeat(64));
        assert!(action_evidence_matches(&replayed, &digests));
        replayed.product_audit_event_id = Some("a".repeat(64));
        assert!(!action_evidence_matches(&replayed, &digests));
        replayed.product_audit_event_id = Some("B".repeat(64));
        assert!(!action_evidence_matches(&replayed, &digests));
        replayed.product_receipt_id = Some("4".repeat(64));
        replayed.product_audit_event_id = Some("b".repeat(64));
        assert!(!action_evidence_matches(&replayed, &digests));
    }

    fn closed_failure_row(outcome_name: &str) -> LifecycleCancellationRow {
        LifecycleCancellationRow {
            outcome_name: outcome_name.to_string(),
            exact_replay: false,
            product_resulting_revision: None,
            product_resulting_state: None,
            guild_id: None,
            product_receipt_id: None,
            product_audit_event_id: None,
            cancellation_reason_digest: None,
            product_operation_id: None,
            source_product_mutation_request_bytes: None,
            product_mutation_digest: None,
            source_drain_intent_request_bytes: None,
            drain_intent_digest: None,
            source_deployment_id: None,
            source_deployment_revision: None,
            source_deployment_snapshot: None,
            source_deployment_snapshot_digest: None,
            source_result_deployment_revision: None,
            source_result_deployment_snapshot: None,
            source_result_deployment_snapshot_digest: None,
            drain_intent_id: None,
            source_intent_revision: None,
            source_state_bytes: None,
            source_state_digest: None,
            result_intent_revision: None,
            result_intent_state: None,
            result_state_bytes: None,
            result_state_digest: None,
            source_slot_epoch: None,
            successor_slot_epoch: None,
            terminal_action_id: None,
            terminal_projection_bytes: None,
            terminal_projection_digest: None,
            terminal_database_time: None,
        }
    }
}
